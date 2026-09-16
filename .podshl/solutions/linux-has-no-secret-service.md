---
id: linux-has-no-secret-service
answers:
  problem_class: podshl.model.key-not-kept
  when:
    os.name: linux
severity: medium
proposes:
  - action: report_only
    because: >-
      Installing and unlocking a keyring is a change to your session, not to
      this application's own settings, and it is not something a support client
      should do on your behalf
---
**The key is asked for and not kept.** This client never writes an API key into
a file of its own. It hands it to the operating system's credential store, so
that a key lives where the rest of your credentials live and is protected the
way they are — and on Linux that store is a Secret Service provider.

With none installed and unlocked there is nowhere to put it, so it is not kept.
Install one and the settings keep the key:

* GNOME Keyring — `gnome-keyring` and `libsecret` on most distributions
* KWallet, on KDE

It has to be **unlocked** in your session, not merely installed. On a machine
you log into graphically that usually happens at login; over SSH or on a
headless session it does not.

Nothing else about the client needs this. Everything except a cloud model's key
works without a keyring at all, including every published answer like this one,
and a model running on your own machine needs no key in the first place.
