---
id: wheel-missing-for-python
answers:
  problem_class: pip.install.wheel-missing
  when:
    python.version: ">= 3.13"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: The fix is to install a different Python, which this client will not do for you
---
There is no wheel for your Python version yet, so pip fell back to building
from source and the build needs a compiler you probably do not have.

This is not a bug in the package. Wheels are published per Python minor
version, and a new Python release always arrives before the wheels for it do.

Install alongside a Python that has wheels — 3.11 or 3.12 today — and point the
project at that one. Nothing needs uninstalling.

Deliberately no automated action here: switching the Python a project runs on
is a decision with consequences this agent cannot see, and an action vocabulary
that could do it would be one that could break a working environment.
