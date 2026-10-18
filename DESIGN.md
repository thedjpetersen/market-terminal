# Web companion audit reference

L00309 uses **Bloomberg information density with Linear keyboard clarity**.
This records review criteria; this audit does not redesign the product.

- Color: preserve the existing restrained dark palette: background `#0b1014`,
  panel `#11181d`, border `#243036`, text `#e9efed`, muted `#93a1a8`,
  accent `#87e6b7`. Keep negative values distinguishable by signs and words.
- Type: system sans for navigation and prose, monospace for numerical evidence.
  Target 12/14/16px supporting text, 20/28/34px headings; essential source and
  status information must remain readable at mobile widths and enlarged text.
  Current mobile styles include 8–10px labels and tracked uppercase eyebrows;
  these need visual review against the Factory design bar.
- Spacing: use a 4/8px rhythm, compact related data, and clear separation between
  instruments, assumptions and results. Do not compress disclosures away.
- Radius: restrained 4–8px controls/panels; modal corners may use 12px.
- Motion: honor reduced-motion preferences for both CSS and scripted scrolling.
- Keyboard: visible focus, one active modal, predictable Escape/Tab behavior,
  programmatic selected states, meaningful chart readouts and route orientation.
- Do: show provider, observation time, unavailable state and recovery together;
  preserve dense numerical comparisons and horizontal scrolling inside tables.
- Do not: imply live freshness with decorative status dots; use color alone
  for selection; hide essential help on mobile; replace missing data with zeros.

Desktop 1440×1000, mobile 390×844 and narrow 320px reflow are review targets.
Light-mode behavior, contrast, zoom, focus visibility and real touch behavior
remain unverified. No screenshot-based design score is assigned: browser launch
was sandbox-blocked and connected-browser local access was denied.
