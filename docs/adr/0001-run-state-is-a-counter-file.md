# A run's state is one small file of counters; everything else is read from git and the forge

Once a run spans several turns, a wait for checks and a restart of the app, it needs some state
beyond the claim. We keep one JSON file per active run, holding only what nothing else records:
the run's phase, how many turns and repairs it has used, and the time it has spent working. The
branch, the worktree, the commits, the pull request and its checks are read from git and the
forge whenever they are needed, and they are always asked for before anything is pushed or a
pull request is created, so a response lost in a crash cannot lead to a duplicate. The file is
deleted when the run ends. The next attempt finds its branch by the stable
`onehand/issue-<N>` prefix, and finds its pull request through the forge.

## Considered options

- **SQLite with a schema, migrations and an outbox for forge writes.** This was rejected. It adds
  a dependency and a second record of facts that git and the forge already hold, and every
  recovery then has to reconcile the two. A single process with one tick makes leases and
  ownership generations unnecessary.
- **No local state, with the phase carried in issue labels.** This was rejected. Turn and repair
  counters have nowhere to live on an issue, and label churn is visible to everyone who watches
  the repository.
