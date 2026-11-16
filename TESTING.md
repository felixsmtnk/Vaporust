# How Vaporust is tested

**English** · [Русский](TESTING.ru.md)

## Automated tests

They run on every build in GitHub Actions (`cargo test`), together with the tests of the Aurelia library.

- **Parsing Steam pages** on saved samples (`testdata/steam`, captured without logging in):
  a profile, a group, discussions, a guide, a Workshop mod, reviews, curators, store data,
  market search, GitHub releases. Plus junk and truncated pages: parsing must not crash.
- **Damaged Steam files**: appmanifest, libraryfolders.vdf, localconfig.vdf, shortcuts.vdf.
  Vaporust must not rewrite them if it could not read them.
- **Save backups**: an unavailable or damaged file stops the restore before anything is
  replaced, and a failure in the middle rolls back what was already replaced.
- **Update archive**: junk, no exe, a wrong format, a different bitness - the old exe is not touched.
- **Market prices** by the rules of Steam currencies (minimum price, step, fees).
- **Account switching**: answers of the previous login do not get into the new one.
- **Downloads**: disk space calculation, whether to continue after a network error, conflicts
  of the temporary folder.
- **Experimental features**: the list of actions that do not run without the switch.

Not simulated: a real connection loss in the middle of a download, and the PC turning off in the
middle of restoring saves (for that case a safety backup is made before restoring).

## Weekly check of Steam pages

Once a week GitHub Actions (`steam-pages.yml`) downloads fresh public Steam pages (without
logging in) and runs the parsing tests on them. If Valve changed the markup and parsing broke,
an issue is opened automatically: the problem is found before users notice it.

## Live tests

They are marked `#[ignore]` and run by hand (`cargo test <name> -- --ignored`): they go to the
Steam network with the saved login, only read and publish nothing. Downloads are tested only on
games that are not in the libraries.

## Manual testing

Every change is checked in the program itself on two Windows 10 PCs: a normal PC (NVIDIA) and an
old ThinkPad E330 laptop (Intel graphics). What is little tested - in the README, "What is tested".
