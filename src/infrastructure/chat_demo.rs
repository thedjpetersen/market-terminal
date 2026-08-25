use std::sync::Mutex;

use crate::features::chat::{
    validate_chat_message, ChatConnectionState, ChatEndpoint, ChatEvent, ChatGateway,
    ChatGatewayError, ChatMessage, ChatMessageKind,
};

pub struct DemoChatGateway {
    events: Mutex<Vec<ChatEvent>>,
    endpoint: ChatEndpoint,
    sequence: Mutex<u64>,
}

impl DemoChatGateway {
    pub fn new() -> Self {
        let endpoint = ChatEndpoint {
            server: "irc.demo.market".to_owned(),
            port: 6697,
            tls: true,
            nickname: "you".to_owned(),
            channel: "#macro-desk".to_owned(),
            configured: true,
        };
        let messages = [
            ("08:31", "rates", "US 10Y +4.2bp; curve bear-steepening into the open."),
            ("08:33", "equity-desk", "Semis leading premarket. Watching breadth after cash open."),
            ("08:35", "fx", "DXY 103.42; EURUSD holding 1.0840 support."),
            ("08:38", "macro", "Consensus still sees the next cut in September."),
            ("08:41", "market-bot", "ALERT · NVDA crossed 125.00 on above-average volume."),
        ];
        let mut events = vec![
            ChatEvent::State(ChatConnectionState::Connected),
            ChatEvent::Status("DEMO FEED · CONFIGURE IRC_SERVER FOR LIVE CHAT".to_owned()),
        ];
        events.extend(messages.into_iter().enumerate().map(|(index, (time, sender, body))| {
            ChatEvent::Message(ChatMessage {
                sequence: index as u64 + 1,
                time: time.to_owned(),
                sender: sender.to_owned(),
                target: endpoint.channel.clone(),
                body: body.to_owned(),
                kind: if sender == "market-bot" {
                    ChatMessageKind::Notice
                } else {
                    ChatMessageKind::Message
                },
                own: false,
            })
        }));
        Self { events: Mutex::new(events), endpoint, sequence: Mutex::new(5) }
    }
}

impl Default for DemoChatGateway {
    fn default() -> Self { Self::new() }
}

impl ChatGateway for DemoChatGateway {
    fn endpoint(&self) -> ChatEndpoint { self.endpoint.clone() }

    fn drain_events(&self) -> Vec<ChatEvent> {
        self.events.lock().expect("demo chat events lock").drain(..).collect()
    }

    fn send_message(&self, message: &str) -> Result<(), ChatGatewayError> {
        let message = validate_chat_message(message)?;
        let mut sequence = self.sequence.lock().expect("demo chat sequence lock");
        *sequence += 1;
        self.events.lock().expect("demo chat events lock").push(ChatEvent::Message(
            ChatMessage {
                sequence: *sequence,
                time: "NOW".to_owned(),
                sender: self.endpoint.nickname.clone(),
                target: self.endpoint.channel.clone(),
                body: message.to_owned(),
                kind: ChatMessageKind::Message,
                own: true,
            },
        ));
        Ok(())
    }

    fn reconnect(&self) -> Result<(), ChatGatewayError> {
        self.events
            .lock()
            .expect("demo chat events lock")
            .extend([ChatEvent::State(ChatConnectionState::Reconnecting), ChatEvent::State(
                ChatConnectionState::Connected,
            )]);
        Ok(())
    }
}
