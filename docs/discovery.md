# rusty-psn — Discovery

> **Purpose — so that later, anyone can tell whether the thing being
> built is still the thing that was wanted.**

This is what everything is checked against for the life of the project.
`~/.claude/workflow.md` § 3 owns what belongs here and how its gate passes.

**Status:** agreed (2026-10-12). The owner confirmed the *never* item on its own, and agreed S10 the same day.

## The problem

Finding and installing official game updates for PlayStation games by hand
is slow and error-prone. It means looking up game IDs, downloading files
one at a time, checking them and installing them.

## Who it is for

- A person who wants the official PSN updates for a PS3, PS4 or PSVita game,
  whatever they will use them for.
- A person who plays PlayStation games in the RPCS3 emulator and wants their
  updates found, checked and installed for them. The owner is one of these.

## Signs it is working

- **S1** — For a game ID, every update Sony lists downloads in full and
  matches the checksum Sony provides. A file that fails the check is never kept.
- **S2** — When Sony's servers drop the connection mid-download, the download
  picks up where it stopped and still ends verified.
- **S3** — A PS4 update that Sony serves in several parts ends up as one
  complete package file.
- **S4** — Downloads land only inside the folder the user chose, laid out as
  Game Title / Game ID / files.
- **S5** — A PS3, a PS4 and a PSVita game ID each return their update list,
  and S1 holds for each.
- **S6** — An update downloaded for a PS3 game installs into RPCS3, and RPCS3
  then shows the game at the new version.
- **S7** — A scan of the RPCS3 game library marks every installed game
  correctly as up to date or needing an update.
- **S8** — It finds the RPCS3 folder without the user typing a path, and the
  user can still change it.
- **S9** — The window app and the command line can each look up, download
  and verify updates on Linux, Windows and macOS.
- **S10** — A DLC zip file the user already has installs into RPCS3, and the
  game then sees the DLC.

## What it deliberately does not do

- **Never:** download full games, or anything that needs a purchase. Updates,
  patches and firmware only.
