# Security

eDock runs a virtual machine, containers that can be privileged, and a local
certificate authority on your computer (macOS or Linux), so security
reports matter a lot.

## Reporting a vulnerability

Please do not open a public issue. Use GitHub's private vulnerability
reporting on this repository (Security → Report a vulnerability).

Include what you found, how to reproduce it, and the eDock version
(the Server version `docker version` shows). You should get a reply within a few days,
and a fix or a plan once the issue is confirmed. Reporters are credited in
the release notes unless they prefer not to be.

## Supported versions

Security fixes go into the latest release; please update before reporting.

## In scope

- Escaping a container or the VM to the host (the Mac or the Linux machine).
- The daemon socket, the HTTP/HTTPS router, DNS, and the certificate
  authority (it is limited to `edock.local` and `edock.localhost` names).
- The installer and `edock setup`.
