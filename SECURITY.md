# Security

**English** · [Русский](SECURITY.ru.md)

## Reporting a vulnerability

If you found a vulnerability (leaking login data, running foreign code, a replaced update and so on),
please **do not open a public issue**. Report it privately: the **Security** tab of this
repository -> **Report a vulnerability**. Describe what you found and how to reproduce it.

Ordinary bugs (something does not work or shows wrong) go to [Issues](../../issues).

## Supported versions

Fixes are released only in the latest version on the [Releases](../../releases) page.
Vaporust is an alpha, old versions are not updated.

## Verifying a downloaded archive

- Next to every archive there is a `.sha256` file with its checksum. To check it in PowerShell:
  `Get-FileHash Vaporust-...-windows-x64.zip -Algorithm SHA256` - the number must match.
  The built-in self-update does this check itself and replaces nothing without a match.
  The checksum lies in the same release as the archive: it protects from a broken download, not
  from someone who took over the GitHub account and replaced both files. For that check the
  provenance attestation below (the self-update does not check it yet).
- The archives are built by GitHub Actions from the code of this repository with a build
  provenance attestation. You can verify it with the GitHub CLI:
  `gh attestation verify Vaporust-...-windows-x64.zip --repo felixsmtnk/Vaporust`.
- GitHub Actions are pinned to exact versions (commit SHA), the build has read-only permissions;
  only the last step (the draft release) can write.

## What Vaporust does with your data

- **The Steam password** is not stored. It goes only to Steam servers at login (the same as in
  the normal client). You may not enter a password at all: log in with a QR code from the Steam
  app, or use the "Without login" mode.
- **The saved login** (a session key, not the password) is kept encrypted in
  `%USERPROFILE%\.config\Aurelia\session.json`; the encryption key is in the Windows Credential
  Manager. Its state is shown in "Settings -> Account".
- **No telemetry.** Vaporust talks only to Steam (Valve) servers and to GitHub (update check,
  which can be turned off).
- **The error log** (`%APPDATA%\Vaporust\vaporust.log`) is written without the login, SteamID,
  Windows user name and login keys.

## Revoking access

- Log out in Vaporust: the "Vaporust" menu -> "Log out" (the saved login is deleted).
- Revoke all logins on the Steam side: [store.steampowered.com/account/authorizeddevices](https://store.steampowered.com/account/authorizeddevices)
  or "Deauthorize all other devices" in the Steam Guard settings.
- Before uninstalling Vaporust: "Settings -> About -> Remove Vaporust changes in Steam".
