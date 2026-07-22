# Code signing

Goal: signed release binaries so Windows SmartScreen stops flagging the
installer as coming from an "unknown publisher" — at zero cost, since
MCP Hangar is free and open source.

## How it works

`release.yml` contains optional signing steps powered by the
[SignPath Foundation](https://signpath.org) (free code signing for qualifying
OSS projects, running on [SignPath.io](https://signpath.io)). The steps are
skipped while the `SIGNPATH_API_TOKEN` secret is absent, so releases keep
working unsigned until the one-time setup below is done.

With signing enabled, a release does:

1. Build `mcp-hangar.exe` (x86 MSVC)
2. Sign the exe via SignPath
3. Compile the Inno Setup installer, embedding the already-signed exe
4. Sign the installer via SignPath
5. Publish installer + portable zip (both containing signed binaries)

Note: the publisher shown by Windows will be **SignPath Foundation** (the
certificate is theirs, issued on behalf of qualifying OSS projects), not
"Software Imperial". SmartScreen warnings fade as the certificate's existing
reputation applies.

## One-time setup

1. Apply at <https://signpath.org> ("Get OSS code signing"). The project
   qualifies: MIT (OSI) license, no proprietary components, actively
   maintained, already released, functionality documented in the README.
   Point the application at this repo and its release workflow.
2. After approval, in the SignPath dashboard:
   - Add the predefined **GitHub.com** trusted build system to the
     organization and link it to the project.
   - Make sure the project slug is `mcp-hangar` and the signing policy slug
     is `release-signing` (they must match `release.yml`), with a PE-file
     artifact configuration.
   - Create an API token for a CI user with submitter permission.
3. In the GitHub repo settings:
   - Actions **secret** `SIGNPATH_API_TOKEN` — the API token
   - Actions **variable** `SIGNPATH_ORGANIZATION_ID` — the organization id
4. Tag a release. If the signing policy requires manual approval, approve the
   two signing requests (exe, then installer) in the SignPath dashboard while
   the workflow waits (up to 30 min each).
5. Add the SignPath Foundation attribution to the README — they require a
   mention on the project page once signing is live.

## Until signing is live

- The README carries a SmartScreen notice (**More info → Run anyway**).
- Optionally, report each release installer to Microsoft as an incorrectly
  flagged file to speed up per-file reputation:
  <https://www.microsoft.com/en-us/wdsi/filesubmission> (sign in, submit as
  "Software developer"). Reputation is per file hash, so it resets on every
  release — signing is the real fix.
