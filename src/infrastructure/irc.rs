use std::{
    env,
    sync::{
        Mutex,
        mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_util::StreamExt;
use irc::client::prelude::{Client, Command, Config};

use crate::features::chat::{
    ChatConnectionState, ChatEndpoint, ChatEvent, ChatGateway, ChatGatewayError, ChatMessage,
    ChatMessageKind, MAX_CHAT_EVENTS_PER_POLL, validate_chat_message,
};

const COMMAND_CAPACITY: usize = 64;
const EVENT_CAPACITY: usize = 512;
const RECONNECT_DELAY: Duration = Duration::from_secs(5);

enum WorkerCommand {
    Send(String),
    Reconnect,
}

enum ConnectionExit {
    Reconnect,
    Disconnected(String),
    Shutdown,
}

pub struct IrcChatGateway {
    endpoint: ChatEndpoint,
    commands: Option<SyncSender<WorkerCommand>>,
    events: Mutex<Receiver<ChatEvent>>,
}

impl IrcChatGateway {
    pub fn from_env() -> Self {
        let endpoint = endpoint_from_env();
        let (event_sender, event_receiver) = mpsc::sync_channel(EVENT_CAPACITY);

        if !endpoint.configured {
            return Self {
                endpoint,
                commands: None,
                events: Mutex::new(event_receiver),
            };
        }
        if let Err(error) = endpoint.validate() {
            let _ = event_sender.try_send(ChatEvent::Status(error.to_string()));
            let _ = event_sender.try_send(ChatEvent::State(ChatConnectionState::Disabled));
            return Self {
                endpoint,
                commands: None,
                events: Mutex::new(event_receiver),
            };
        }

        let server_password = env::var("IRC_SERVER_PASSWORD").ok();
        let nick_password = env::var("IRC_NICKSERV_PASSWORD").ok();
        let (command_sender, command_receiver) = mpsc::sync_channel(COMMAND_CAPACITY);
        let worker_endpoint = endpoint.clone();
        let spawn = thread::Builder::new()
            .name("market-terminal-irc".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(run_worker(
                        worker_endpoint,
                        server_password,
                        nick_password,
                        command_receiver,
                        event_sender,
                    )),
                    Err(error) => {
                        let _ = event_sender
                            .try_send(ChatEvent::Status(format!("IRC RUNTIME FAILED · {error}")));
                    }
                }
            });

        if spawn.is_err() {
            return Self {
                endpoint,
                commands: None,
                events: Mutex::new(event_receiver),
            };
        }
        Self {
            endpoint,
            commands: Some(command_sender),
            events: Mutex::new(event_receiver),
        }
    }

    fn queue(&self, command: WorkerCommand) -> Result<(), ChatGatewayError> {
        let Some(commands) = &self.commands else {
            return Err(ChatGatewayError::Disabled);
        };
        commands.try_send(command).map_err(|error| match error {
            TrySendError::Full(_) => ChatGatewayError::Busy,
            TrySendError::Disconnected(_) => ChatGatewayError::Disconnected,
        })
    }
}

impl Default for IrcChatGateway {
    fn default() -> Self {
        Self::from_env()
    }
}

impl ChatGateway for IrcChatGateway {
    fn endpoint(&self) -> ChatEndpoint {
        self.endpoint.clone()
    }

    fn drain_events(&self) -> Vec<ChatEvent> {
        let events = self.events.lock().expect("IRC event queue lock");
        (0..MAX_CHAT_EVENTS_PER_POLL)
            .map_while(|_| events.try_recv().ok())
            .collect()
    }

    fn send_message(&self, message: &str) -> Result<(), ChatGatewayError> {
        let message = validate_chat_message(message)?;
        self.queue(WorkerCommand::Send(message.to_owned()))
    }

    fn reconnect(&self) -> Result<(), ChatGatewayError> {
        self.queue(WorkerCommand::Reconnect)
    }
}

fn endpoint_from_env() -> ChatEndpoint {
    let Ok(server) = env::var("IRC_SERVER") else {
        return ChatEndpoint::offline();
    };
    if server.trim().is_empty() {
        return ChatEndpoint::offline();
    }
    let tls = env_bool("IRC_TLS", true);
    let default_port = if tls { 6697 } else { 6667 };
    ChatEndpoint {
        server: server.trim().to_owned(),
        port: env::var("IRC_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(default_port),
        tls,
        nickname: env::var("IRC_NICKNAME").unwrap_or_else(|_| "market-terminal".to_owned()),
        channel: env::var("IRC_CHANNEL").unwrap_or_else(|_| "#market-terminal".to_owned()),
        configured: true,
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    env::var(name)
        .ok()
        .map(|value| !matches!(value.to_ascii_lowercase().as_str(), "0" | "false" | "no"))
        .unwrap_or(default)
}

async fn run_worker(
    endpoint: ChatEndpoint,
    server_password: Option<String>,
    nick_password: Option<String>,
    commands: Receiver<WorkerCommand>,
    events: SyncSender<ChatEvent>,
) {
    let mut reconnecting = false;
    loop {
        emit(
            &events,
            ChatEvent::State(if reconnecting {
                ChatConnectionState::Reconnecting
            } else {
                ChatConnectionState::Connecting
            }),
        );
        let exit = run_connection(
            &endpoint,
            server_password.clone(),
            nick_password.clone(),
            &commands,
            &events,
        )
        .await;
        reconnecting = true;
        match exit {
            ConnectionExit::Shutdown => break,
            ConnectionExit::Reconnect => continue,
            ConnectionExit::Disconnected(error) => {
                emit(&events, ChatEvent::State(ChatConnectionState::Disconnected));
                emit(
                    &events,
                    ChatEvent::Status(format!("IRC DISCONNECTED · {error}")),
                );
            }
        }

        let mut elapsed = Duration::ZERO;
        while elapsed < RECONNECT_DELAY {
            match commands.try_recv() {
                Ok(WorkerCommand::Reconnect) => break,
                Ok(WorkerCommand::Send(_)) | Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => return,
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            elapsed += Duration::from_millis(100);
        }
    }
}

async fn run_connection(
    endpoint: &ChatEndpoint,
    server_password: Option<String>,
    nick_password: Option<String>,
    commands: &Receiver<WorkerCommand>,
    events: &SyncSender<ChatEvent>,
) -> ConnectionExit {
    let config = Config {
        nickname: Some(endpoint.nickname.clone()),
        username: Some(endpoint.nickname.clone()),
        realname: Some("Market Terminal".to_owned()),
        server: Some(endpoint.server.clone()),
        port: Some(endpoint.port),
        password: server_password,
        nick_password,
        use_tls: Some(endpoint.tls),
        channels: vec![endpoint.channel.clone()],
        ..Config::default()
    };
    let mut client = match Client::from_config(config).await {
        Ok(client) => client,
        Err(error) => return ConnectionExit::Disconnected(error.to_string()),
    };
    if let Err(error) = client.identify() {
        return ConnectionExit::Disconnected(error.to_string());
    }
    let current_nickname = client.current_nickname().to_owned();
    let mut stream = match client.stream() {
        Ok(stream) => stream,
        Err(error) => return ConnectionExit::Disconnected(error.to_string()),
    };
    emit(events, ChatEvent::State(ChatConnectionState::Connected));
    emit(
        events,
        ChatEvent::Status(format!("JOINED {} AS {current_nickname}", endpoint.channel)),
    );
    let mut sequence = 0_u64;
    let mut command_poll = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            inbound = stream.next() => match inbound {
                Some(Ok(message)) => {
                    let nickname = message.source_nickname().unwrap_or("server").to_owned();
                    match &message.command {
                        Command::PRIVMSG(target, body) => {
                            sequence += 1;
                            let (kind, body) = parse_message_body(body);
                            emit(events, ChatEvent::Message(ChatMessage {
                                sequence,
                                time: time_label(),
                                sender: nickname,
                                target: target.clone(),
                                body,
                                kind,
                                own: false,
                            }));
                        }
                        Command::NOTICE(target, body) => {
                            sequence += 1;
                            emit(events, ChatEvent::Message(ChatMessage {
                                sequence,
                                time: time_label(),
                                sender: nickname,
                                target: target.clone(),
                                body: body.clone(),
                                kind: ChatMessageKind::Notice,
                                own: false,
                            }));
                        }
                        Command::JOIN(_, _, _) => emit(
                            events,
                            ChatEvent::ParticipantJoined(nickname),
                        ),
                        Command::PART(_, _) | Command::QUIT(_) => {
                            emit(events, ChatEvent::ParticipantLeft(nickname));
                        }
                        _ => {}
                    }
                }
                Some(Err(error)) => return ConnectionExit::Disconnected(error.to_string()),
                None => return ConnectionExit::Disconnected("server closed the stream".to_owned()),
            },
            _ = command_poll.tick() => loop {
                match commands.try_recv() {
                    Ok(WorkerCommand::Send(body)) => {
                        if let Err(error) = client.send_privmsg(&endpoint.channel, &body) {
                            return ConnectionExit::Disconnected(error.to_string());
                        }
                        sequence += 1;
                        emit(events, ChatEvent::Message(ChatMessage {
                            sequence,
                            time: time_label(),
                            sender: current_nickname.clone(),
                            target: endpoint.channel.clone(),
                            body,
                            kind: ChatMessageKind::Message,
                            own: true,
                        }));
                    }
                    Ok(WorkerCommand::Reconnect) => {
                        let _ = client.send_quit("reconnecting");
                        return ConnectionExit::Reconnect;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        let _ = client.send_quit("terminal closed");
                        return ConnectionExit::Shutdown;
                    }
                }
            }
        }
    }
}

fn parse_message_body(body: &str) -> (ChatMessageKind, String) {
    body.strip_prefix("\u{1}ACTION ")
        .and_then(|action| action.strip_suffix('\u{1}'))
        .map(|action| (ChatMessageKind::Action, action.to_owned()))
        .unwrap_or_else(|| (ChatMessageKind::Message, body.to_owned()))
}

fn time_label() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    format!("{:02}:{:02}", seconds / 3_600, (seconds % 3_600) / 60)
}

fn emit(events: &SyncSender<ChatEvent>, event: ChatEvent) {
    let _ = events.try_send(event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctcp_actions_are_presented_without_wire_markers() {
        assert_eq!(
            parse_message_body("\u{1}ACTION checks the tape\u{1}"),
            (ChatMessageKind::Action, "checks the tape".to_owned())
        );
    }
}
