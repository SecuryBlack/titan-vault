---
name: Bug report
about: Create a report to help us improve TitanVault
title: '[BUG] '
labels: 'bug'
assignees: ''

---

**Describe the bug**
A clear and concise description of what the bug is.

**To Reproduce**
Steps to reproduce the behavior:
1. Agent configuration used (`config.toml` with passphrases, tokens and storage keys redacted)
2. Command run / Service state (`systemctl status titanvault` or `Get-Service TitanVault`)
3. See error in logs (`journalctl -u titanvault` or Event Viewer)

**Expected behavior**
A clear and concise description of what you expected to happen.

**Environment (please complete the following information):**
 - TitanVault Version: [e.g. 0.2.3]
 - OS & Architecture: [e.g. Ubuntu 22.04 LTS x86_64, Windows Server 2022 AMD64]
 - Mode: [e.g. standalone, securyblack]
 - Storage target: [e.g. Hetzner, Cloudflare R2, S3-compatible, local]

**Logs / Screenshots**
If applicable, attach relevant log snippets from `titanvault`.

**Additional context**
Add any other context about the problem here.
