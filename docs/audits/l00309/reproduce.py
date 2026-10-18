"""Create ignored port-adjusted copies; never edit the existing acceptance tests."""
from pathlib import Path

root = Path(__file__).resolve().parents[3]
web = root / "web"
host = (web / "tests/local-host.mjs").read_text()
host = host.replace("8900", "42000").replace("8899", "41999")
host = host.replace('"--port",', '"--inspector-port",\n    "42001",\n    "--port",')
(web / ".l00309-host.mjs").write_text(host)
for name in ("engine", "workflows"):
    source = (web / f"tests/{name}.browser.ts").read_text()
    (web / f"tests/l00309-{name}.browser.ts").write_text(
        source.replace("8900", "42000")
    )
config = (web / "playwright.config.ts").read_text()
config = config.replace("8899", "41999").replace("8900", "42000")
config = config.replace("**/*.browser.ts", "**/l00309-*.browser.ts")
config = config.replace("node tests/local-host.mjs", "node .l00309-host.mjs")
# The audit started the same local issuer/Worker separately and reused it.
config = config.replace("reuseExistingServer: false", "reuseExistingServer: true")
(web / ".l00309-playwright.config.ts").write_text(config)
print("Created ignored local audit copies on ports 41999, 42000, 42001.")
