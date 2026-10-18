# Search interaction reference

L00372 uses Linear's restrained command-search interaction as a behavior
reference and preserves the existing Market Terminal visual system.

- Colors: background `#0b1014`, panel `#11181d`, border `#243036`, text
  `#e9efed`, muted `#93a1a8`, accent `#87e6b7`.
- Type: existing system/Inter stack; search input 15px, ticker 12px,
  company and keyboard hints 10px.
- Spacing: existing 4/8px rhythm with 12px row gaps and 16/20px field padding.
- Radius: 12px dialog and 5px result rows.
- Motion: retain existing 160ms interaction transitions and reduced-motion
  rules; add no motion for request transitions.
- Do show a pending state immediately after an edit, an explicit failure, or
  a successful empty state. Only current successful results are actionable.
- Do preserve keyboard/pointer parity and desktop/mobile geometry. Do not
  add layout, typography, modal-stack, or focus-management changes in this fix.
