# What a maintainer gets, beyond the answer

Two things PODSHL does not do yet, written down as stated plans rather than
silences — and why the obvious ways of building them are wrong. The short
version is in [the README](../README.md#for-maintainers); this is the
reasoning.

## Your own ticket system

Today a diagnosis that nothing published covers ends as **Markdown you take to
the project**: anonymised, editable, shown before it is copied, with the
address from your `escalate.target` beside it. That works with every tracker
there is — GitHub, GitLab, Jira, Redmine, Zammad, your own — because the
thing carrying it is a person with a clipboard, and a person needs no
integration.

**An automatic path into your tracker is planned and not built.** If you run
your own ticket system you already have the infrastructure that would make it
worth doing, and "copy this into the browser" is a worse answer for you than
for a project whose tracker is a GitHub tab. It is not built yet because the
obvious ways of building it are all wrong:

* a desktop client holding an API token for your tracker would be standing
  credentials into your systems, on every user's machine;
* relaying through the operator would make it a party to the content **and**
  give it credentials to third-party trackers.

Neither is a thing this project will ship. What it will look at is the shape
that keeps the reporter in the loop — they still press send, nothing holds a
credential it should not, and you receive a case rather than a paste. Until
then, running an A2A endpoint yourself is the supported automatic path, and
`spec/INTEGRATING.md` says what it has to answer.

## "This works, but it should do X" — later

Everything here is built around something being **wrong**: a problem class, a
symptom a person recognises, readings that decide between published answers, an
outcome saying whether the fix worked. A person who thinks your software should
do something it does not have any of that. There is no symptom to read, nothing
on their machine decides anything, and the report that would be assembled is a
report about a machine that is behaving exactly as designed.

**A feature request is a different thing and will get a different path.** It is
written down here so it is a stated plan rather than a silence, and it is
deliberately not next: the diagnosis path is not finished, and the two obvious
shortcuts would spoil what exists.

* Filing it as an outcome would put opinions into a corpus whose value is that
  every row is a measurement. `uncovered` already means *nothing published
  covers this* — adding *and it never will, because it is not a defect* to the
  same table would make the maintainer's dashboard a wish list with readings
  attached.
* Letting a model turn a wish into a bug report is worse than nothing. It
  produces a plausible issue about a defect that does not exist, and somebody
  has to close it.

What it probably looks like: the person's own words, no readings at all, the
same anonymiser and the same consent, counted per project the way reports are
counted so a maintainer can see that forty people asked for the same thing —
and never mixed with the diagnoses. Nothing is designed yet.
