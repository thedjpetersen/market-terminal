"""Generate port-only copies of the existing browser acceptance suite.

Run from any directory. Original test assertions, fixtures and product code stay
unchanged. Derived from L00309's adapter at commit 8b223c9.
"""
from pathlib import Path

root = Path(__file__).resolve().parents[3]
web = root / "web"
host = (web / "tests/local-host.mjs").read_text()
host = host.replace("8900", "42000").replace("8899", "41999")
host = host.replace('"--port",', '"--inspector-port",\n    "42001",\n    "--port",')
(web / ".l00371-host.mjs").write_text(host)
for name in ("engine", "workflows"):
    source = (web / f"tests/{name}.browser.ts").read_text()
    (web / f"tests/l00371-{name}.browser.ts").write_text(
        source.replace("8900", "42000")
    )
config = (web / "playwright.config.ts").read_text()
config = config.replace("8899", "41999").replace("8900", "42000")
config = config.replace("**/*.browser.ts", "**/l00371-*.browser.ts")
config = config.replace("node tests/local-host.mjs", "node .l00371-host.mjs")
(web / ".l00371-playwright.config.ts").write_text(config)
print("Created ignored acceptance copies on ports 41999, 42000, 42001.")
