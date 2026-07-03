# Implementation plan and status

### Step 1: Team data model and slash commands

Add team state to the daemon's persistent state. Implement `/jamsession:join-team`, `/jamsession:leave-team`, `/jamsession:teams` slash commands. Implement context injection on join.

- [ ] TBD

### Step 2: `list-members`

Add the `ListMembers` variant and handler.

- [ ] TBD

### Step 3: Messaging (`broadcast` + `send`)

Implement message queuing, delivery on next turn, and wake-on-message for idle agents.

- [ ] TBD

### Step 4: Worklist (`post-worklist`, `remove-worklist`, `show-worklist`)

- [ ] TBD

### Step 5: Key-value store (`store`, `retrieve`)

- [ ] TBD
