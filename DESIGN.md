# Web interaction reference

Reference: Linear's restrained workspace interactions. This task preserves the existing Market Terminal appearance and improves modal ownership and keyboard predictability.

- Colors: background `#0b1014`, panel `#11181d`, border `#243036`, text `#e9efed`, muted `#93a1a8`, accent `#87e6b7`.
- Typography: existing Inter/system stack; 11–13px supporting text, 16px mobile search input, existing heading scale.
- Spacing: preserve existing 4/8px rhythm, 24px desktop backdrop inset and 14px mobile inset.
- Radius: 6px inputs and 12px dialogs.
- Motion: retain existing 160ms control transitions and reduced-motion override; no new modal animation.
- Do keep one active dialog, visible focus, keyboard access to evidence, deliberate dismissal and trigger restoration. Do not stack dialogs, move focus to another dialog or reset page scroll when returning focus.
