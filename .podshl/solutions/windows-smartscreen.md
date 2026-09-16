---
id: windows-smartscreen
answers:
  problem_class: podshl.start.refused-by-the-system
  when:
    os.name: windows
severity: medium
proposes:
  - action: report_only
    because: >-
      Deciding to run an unsigned program is yours to make, and a support tool
      that clicked through a security warning for you would be the wrong kind
      of helpful
---
**"Windows protected your PC".** Nothing is wrong with the download. The
installer is not code-signed, so SmartScreen has no publisher to name and says
so in the strongest words it has.

**More info → Run anyway.**

The setup installs for your user only — no administrator, no UAC prompt — under
`%LOCALAPPDATA%\PODSHL`.

Before you do that, you are entitled to check that the file arrived intact:
`SHA256SUMS` in the release lists every file's digest, and `certutil -hashfile
<file> SHA256` prints the one you have. That says the file is the one that was
published; it does not say who published it, and only a signature would. There
is no certificate yet, which is a decision with a price rather than an oversight
— it is written down in `RELEASING.md`.
