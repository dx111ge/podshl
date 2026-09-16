---
id: macos-quarantine
answers:
  problem_class: podshl.start.refused-by-the-system
  when:
    os.name: macos
severity: medium
proposes:
  - action: report_only
    because: >-
      Removing the quarantine attribute is a decision about trusting an
      unsigned program, and it is yours — this client does not change
      attributes on files it did not write
---
**macOS says the app is damaged, or from an unidentified developer.** It is
neither. Everything downloaded from a browser carries a quarantine attribute,
and for an unsigned application macOS refuses it outright rather than asking.

Once, in Terminal:

    xattr -dr com.apple.quarantine /Applications/PODSHL.app

You are entitled to check the file first: `SHA256SUMS-macos` in the release
lists the digests, and `shasum -a 256 <file>` prints yours.

One thing said plainly: **this build has never been run by us.** There is no Mac
here. It is built on GitHub's macOS runners and published untested, which is why
a report of how it went is worth more from this platform than from either of the
others.
