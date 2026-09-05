#!/usr/bin/env python3
"""Real native PTY + HTTP adapter + disposable disk E2E. Requires pyte==0.8.2.

Run: python3 scripts/news_e2e.py --binary target/debug/market-terminal --evidence DIR
External publisher responses are synthetic; app adapters and persistence are real.
Only synthetic screen text and allowlisted HTTP/step metadata are retained.
"""
import argparse
import codecs
import datetime as dt
import fcntl
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import tempfile
import termios
import threading
import time
import pyte


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--evidence', required=True, type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=True)
    run_id = dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    events = []

    def event(kind, **values):
        events.append(dict(utc=dt.datetime.now(dt.timezone.utc).isoformat(),
                           run_id=run_id, kind=kind, **values))

    class Publisher(BaseHTTPRequestHandler):
        article_attempts = 0

        def log_message(self, *_):
            pass

        def do_GET(self):
            start = time.monotonic()
            status = 200
            route = self.path.split('?')[0]
            if route in ['/alpha.xml', '/beta.xml']:
                alpha = route == '/alpha.xml'
                wire = 'Alpha Wire' if alpha else 'Beta Wire'
                title = 'Alpha synthetic research' if alpha else 'Beta synthetic report'
                article_path = '/article' if alpha else '/beta-article'
                date = 'Sat, 05 Sep 2026 20:00:00 GMT' if alpha else 'Sat, 05 Sep 2026 19:00:00 GMT'
                body = f'<rss version="2.0"><channel><title>{wire}</title><link>http://127.0.0.1:{self.server.server_port}</link><description>Synthetic fixture</description><item><guid>{wire}-story</guid><title>{title}</title><link>http://127.0.0.1:{self.server.server_port}{article_path}</link><pubDate>{date}</pubDate><description>A short synthetic publisher excerpt for terminal validation.</description></item></channel></rss>'
                mime = 'application/rss+xml'
            elif route == '/article':
                Publisher.article_attempts += 1
                status = 503 if Publisher.article_attempts == 1 else 200
                paragraph = 'Recovered synthetic article body. This fixture describes a fictional research workflow, with repeatable evidence and no financial recommendation. '
                body = '<html><head><title>Alpha synthetic research</title></head><body><article><h1>Alpha synthetic research</h1>' + ''.join('<p>'+paragraph*3+'</p>' for _ in range(8)) + '</article></body></html>'
                mime = 'text/html'
            else:
                status, body, mime = 404, 'Unavailable synthetic route', 'text/plain'
            data = body.encode()
            self.send_response(status)
            self.send_header('Content-Type', mime)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            event('publisher_http', route=route if route in ['/alpha.xml','/beta.xml','/article'] else 'other', status=status, duration_ms=round((time.monotonic()-start)*1000, 2))

    class Terminal:
        def __init__(self, env, cwd):
            self.master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 48, 160, 0, 0))
            self.screen = pyte.Screen(160, 48)
            self.stream = pyte.Stream(self.screen)
            self.decoder = codecs.getincrementaldecoder('utf-8')('replace')
            self.proc = subprocess.Popen([str(binary)], stdin=slave, stdout=slave,
                                         stderr=slave, cwd=cwd, env=env, start_new_session=True)
            os.close(slave)

        def pump(self, seconds=.2):
            end = time.monotonic()+seconds
            while time.monotonic() < end:
                if select.select([self.master], [], [], max(0, end-time.monotonic()))[0]:
                    try:
                        data = os.read(self.master, 65536)
                    except OSError:
                        break
                    if not data:
                        break
                    self.stream.feed(self.decoder.decode(data))

        def text(self):
            return '\n'.join(self.screen.display)

        def send(self, value):
            os.write(self.master, value.encode())
            self.pump(.3)

        def command(self, value):
            self.send('/'+value+'\r')

        def expect(self, label, contains, absent=(), timeout=8):
            start = time.monotonic()
            while time.monotonic()-start < timeout:
                self.pump(.1)
                text = self.text()
                if all(value in text for value in contains) and all(value not in text for value in absent):
                    (evidence/(label+'.txt')).write_text(text)
                    event('ui_step', step=label, outcome='pass', expected=list(contains), duration_ms=round((time.monotonic()-start)*1000))
                    return
            (evidence/(label+'.txt')).write_text(self.text())
            event('ui_step', step=label, outcome='fail', expected=list(contains), duration_ms=round((time.monotonic()-start)*1000))
            raise AssertionError(label)

        def close(self):
            if self.proc.poll() is None:
                self.send('\x03')
                try:
                    self.proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    self.proc.terminate()
                    self.proc.wait(timeout=5)
            os.close(self.master)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Publisher)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    terminal = None
    outcome = 'fail'
    started = time.monotonic()
    disposable_state = tempfile.TemporaryDirectory(prefix='market-news-e2e-')
    try:
        disposable = disposable_state.name
        state = Path(disposable)/'state'
        endpoint = f'http://127.0.0.1:{server.server_port}'
        # Do not inherit credentials, user .env, personal state or custom keymaps.
        env = {key:os.environ[key] for key in ['PATH','SYSTEMROOT'] if key in os.environ}
        env.update(HOME=disposable, TERM='xterm-256color', LANG='en_US.UTF-8',
                   MARKET_TERMINAL_STATE_DIR=str(state),
                   MARKET_TERMINAL_NEWS_FEEDS=endpoint+'/alpha.xml,'+endpoint+'/beta.xml',
                   MARKET_TERMINAL_NEWS_SYMBOL_FEED_URL=endpoint+'/alpha.xml',
                   MARKET_TERMINAL_MARKET_DATA_PROVIDER='alpaca', ALPACA_DATA_URL=endpoint,
                   MARKET_TERMINAL_AI_PROVIDER='openrouter')
        terminal = Terminal(env, disposable)
        terminal.expect('00-startup', ['WELCOME TO MARKET TERMINAL'])
        terminal.send('q')
        terminal.command('NEWS')
        terminal.expect('01-all-publishers', ['Alpha synthetic research', 'Beta synthetic report'])
        terminal.send('p')
        terminal.expect('02-source-filter', ['SOURCE Alpha Wire', 'Alpha synthetic research'], ['Beta synthetic report'])
        terminal.send('v')
        terminal.expect('03-article-rejection', ['FULL TEXT UNAVAILABLE', '503'])
        terminal.send('r')
        terminal.expect('04-article-recovery', ['ARTICLE TEXT EXTRACTED', 'Recovered synthetic article body'])
        terminal.send('\x1b')
        terminal.command('VIEW SAVE Publisher QA')
        terminal.expect('05-view-saved', ['VIEW SAVED', 'Publisher QA'])
        terminal.close()
        terminal = None
        documents = list((state/'documents').rglob('*.json'))
        matching = [p for p in documents if 'Publisher QA' in p.read_text()]
        assert matching, 'Saved view was not written to disposable persistence'
        saved = json.loads(matching[0].read_text())
        assert 'Alpha Wire' in json.dumps(saved)
        event('persistence_readback', outcome='pass', document_count=len(matching), source_filter='Alpha Wire')
        terminal = Terminal(env, disposable)
        terminal.expect('06-startup', ['MARKET TERMINAL RUST'])
        terminal.command('NEWS')
        terminal.expect('06-restarted-feed', ['Alpha synthetic research', 'Beta synthetic report'])
        terminal.command('VIEW RESTORE Publisher QA')
        terminal.expect('07-restored-view', ['SOURCE Alpha Wire', 'Alpha synthetic research'], ['Beta synthetic report'])
        terminal.command('NEWS "--source=Missing Publisher"')
        terminal.expect('08-empty-source', ['SOURCE Missing Publisher', 'P ALL SRC'], ['Alpha synthetic research', 'Beta synthetic report'])
        terminal.send('p')
        terminal.expect('09-empty-source-recovery', ['SOURCE ALL', 'Alpha synthetic research', 'Beta synthetic report'])
        terminal.command('VIEW RESTORE Publisher QA')
        terminal.expect('10-saved-view-retained', ['SOURCE Alpha Wire', 'Alpha synthetic research'], ['Beta synthetic report'])
        outcome = 'pass'
    finally:
        if terminal:
            terminal.close()
        disposable_state.cleanup()
        server.shutdown()
        server.server_close()
        revision = subprocess.check_output(['git','rev-parse','HEAD'], text=True).strip()
        (evidence/'events.json').write_text(json.dumps(events, indent=2))
        summary = dict(run_id=run_id, revision=revision, tracked_source_dirty=bool(subprocess.check_output(['git','diff','HEAD','--name-only'])), binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(), outcome=outcome, duration_seconds=round(time.monotonic()-started,2), environment='local native PTY 160x48; real app/HTTP adapters/filesystem; synthetic publisher boundary', flows=['publisher filter + reader failure/retry + saved view process restart/readback','empty publisher filter rejection/recovery'], samples=1)
        (evidence/'result.json').write_text(json.dumps(summary, indent=2))
        print(json.dumps(summary))


if __name__ == '__main__':
    main()
