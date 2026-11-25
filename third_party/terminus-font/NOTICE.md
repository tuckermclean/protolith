# Terminus Font — attribution

protolith's `font-terminus-*` features embed a **191-glyph subset**
(printable ASCII `0x20–0x7E` and Latin-1 `0xA0–0xFF`) of **Terminus Font**,
by **Dimitar Toshkov Zhekov**, at 12 (regular only), 16, 20, 24 and 32
pixels, regular and bold. It is licensed under the **SIL Open Font
License, Version 1.1**: [`OFL.TXT`](OFL.TXT), copied verbatim from the
upstream release.

- Upstream: <https://terminus-font.sourceforge.net/>
- Version: 4.49.1, release tarball SHA-256
  `d961c1b781627bf417f9b340693d64fc219e0113ad3a3af1a3424c7aa373ef79`
- Extracted by `tools/gen-terminus.py` into `src/font/terminus/*.bin`
  (the tool documents the format and how to regenerate).

The subset, re-encoded into a different format, is a *Modified Version* in
the OFL's terms. protolith does not present it to end users as a typeface
named "Terminus Font" (the Reserved Font Name); the name appears only in
this attribution and in API documentation describing where the glyphs come
from. Programs that ship these glyphs must carry `OFL.TXT` with them
(OFL condition 2), for example alongside their own license notices.
