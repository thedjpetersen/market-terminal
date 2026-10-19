# Research recovery UI

Reference: Linear's restrained, compact workspace controls. This task extends the existing Market Terminal design with inline notices and retry actions; it does not redesign navigation or page layouts.

- Colors: canvas `#0b1014`, panel `#11181d`, border `#243036`, primary text `#e9efed`, muted text `#93a1a8`, accent `#87e6b7`. Existing amber notices distinguish unavailable data from normal content.
- Typography: preserve the current Inter/system stack; page headings 34px, panel headings 17px, body 14px, notices 12px. Use sentence case for new copy.
- Spacing: follow the existing 4/8px rhythm and Notice component's padding and margins. Keep the message beside its affected data panel.
- Radius: existing controls 6px, panels 8px, notices 5px.
- Motion: preserve existing short color transitions and reduced-motion overrides; recovery does not add animation.
- Do: reuse accessible status notices, clear retry verbs, existing focus indicators, and responsive wrapping. Distinguish empty results from failed requests and never imply that absent data means no filings exist.
- Do not: add decorative cards, new accents, fabricated data, disabled-looking retry controls, or change the established theme in this focused fix.
