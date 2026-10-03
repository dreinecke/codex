# Open questions

## Q-101 Upgrade herdr to 0.10+ so hushdex session restore works?

Hushdex now reports pane-agent state to herdr (verified live: idle/working tracking,
`herdr agent get`). Session restore after `herdr session stop`/restart needs herdr 0.10+ —
this machine runs 0.9.1, which ignores resume argvs. The reporter already sends
`hushdex resume --yolo <session-id>`, so nothing changes on our side.

- Upgrade herdr (`herdr update` or channel change) — unlock restore, keep everything else
- Stay on 0.9.1 — state tracking and agent APIs work; restore stays manual
