---
id: resolver-backtracking
answers:
  problem_class: pip.install.version-conflict
  when:
    pip.version: "< 23.1"
severity: low
proposes:
  - action: report_only
    params: {}
    because: Naming the cause is the fix; the command to run is yours to choose
---
pip is backtracking: it is trying older and older versions of a dependency
looking for a combination that satisfies everything, and from the outside that
looks like it has hung.

Before 23.1 the resolver had no backtracking limit, so this could run for a very
long time on a conflict it was never going to solve.

Upgrading pip usually ends it, because newer versions give up and tell you which
requirements actually conflict instead of searching in silence.
