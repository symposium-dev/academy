# Summary

- [What is Jamsession?](./README.md)
- [Quick start guide](./quickstart.md)

# Reference

- [Configuration](./configuration.md)

# RFDs

- [About RFDs](./rfds/README.md)
- [RFD template](./rfds/TEMPLATE/README.md)
  - [Implementation plan and status](./rfds/TEMPLATE/implementation.md)
- [Accepted](./rfds/accepted.md)
  - [The `jamsession` MCP tool](./rfds/jamsession-tool/README.md)
    - [Implementation plan and status](./rfds/jamsession-tool/implementation.md)
  - [Inter-agent communication](./rfds/inter-agent-communication/README.md)
    - [Implementation plan and status](./rfds/inter-agent-communication/implementation.md)
- [Completed](./rfds/completed.md)
  - [RFD process](./rfds/rfd-process/README.md)
    - [Implementation plan and status](./rfds/rfd-process/implementation.md)
  - [Session persistence](./rfds/session-persistence/README.md)
    - [Implementation plan and status](./rfds/session-persistence/implementation.md)
  - [Message trace & debug viewer](./rfds/message-trace/README.md)
    - [Implementation plan and status](./rfds/message-trace/implementation.md)

# Appendices

- [Design and implementation](./design/README.md)
  - [Key sequence diagrams](./design/sequence_diagrams.md)
    - [Flow: new session](./design/flow-new-session.md)
    - [Flow: reconnect (load/resume)](./design/flow-reconnect.md)
    - [Flow: message bridge](./design/flow-message-bridge.md)
    - [Flow: idle spin-down](./design/flow-idle-spindown.md)
    - [Flow: agent crash](./design/flow-agent-crash.md)
    - [Flow: cwd health check](./design/flow-cwd-health.md)
    - [Flow: message trace](./design/flow-message-trace.md)
