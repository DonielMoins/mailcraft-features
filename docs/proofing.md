# Spelling & Grammar

> **Status:** current · **Written:** 2026-11-12 · **Last updated:** 2026-11-12 · **Change:** minor

MailCraft checks the message you are writing as you write it, on this computer:
spelling against **every dictionary you turned on at once**, and grammar through a
[LanguageTool](https://languagetool.org/) server that is, unless you say otherwise, a program
running on your own machine.

English and French in one paragraph need no switching. A word is right when *any* dictionary that
is on knows it:

> Bonjour Pierre, I reviewed les documents hier soir and everything looks correct.

`Bonjour`, `les`, `hier` and `soir` are French; `reviewed`, `documents` and `correct` are English
(and `documents` is both). Nothing is flagged for the mixing itself, and nothing is flagged for
being in the other language: the dictionaries are all consulted for every word, always.

## What it looks like

- A word no dictionary knows is underlined in red; a grammar place the server found is underlined
  in blue.
- **Right click** a flagged word for the words that could stand in its place, then *Add to
  Dictionary*, *Ignore Once*, or *Ignore This Rule*.
- **Review › Proofing › Spelling & Grammar** walks through everything found in the message, one
  place at a time, with a field to type a correction of your own.
- A correction is **one step of the message's own undo** (Ctrl+Z / ⌘Z): the text, the formatting
  and the list it was in are as they were a keystroke ago.
- File › Options › Spelling & Grammar turns each part on or off, picks the dictionaries, keeps
  your own words, and says where grammar comes from.

Both themes are drawn from the window's own colours (`theme::Tokens`): the spelling line is the
theme's red and the grammar line its blue, never a colour the sender of a message chose.

## Dictionaries

A dictionary is a [Hunspell](https://manpages.debian.org/hunspell) `.dic`/`.aff` pair already on
your computer — of the words only; no dictionary engine is shipped, and none is downloaded.

| System | Install | Where MailCraft looks |
|---|---|---|
| Arch | `sudo pacman -S hunspell-en_ca hunspell-fr` | `/usr/share/hunspell` |
| Debian/Ubuntu | `sudo apt install hunspell-en-ca hunspell-fr` | `/usr/share/hunspell`, `/usr/share/myspell/dicts` |
| Fedora | `sudo dnf install hunspell-en hunspell-fr` | `/usr/share/hunspell` |
| FreeBSD | `pkg install en-hunspell fr-hunspell` | `/usr/local/share/hunspell` |
| macOS | the system's own, or LibreOffice's dictionaries | `/Library/Spelling`, `~/Library/Spelling` |
| Windows | LibreOffice's dictionaries | `%APPDATA%\MailCraft\dictionaries` |

Your own folder can be named in File › Options › Spelling & Grammar, or in a script:

```
mailcraft-cli run --sample --cmd proof.set --params '{"dictionaryDir": "/home/me/dicts"}'
```

What is looked for, in order, for each language: `en_CA.dic`, `en-CA.dic`, `en_ca.dic`, `en.dic`,
then the same for the other English variant (a Canadian English dictionary stands in with American
English when there is no Canadian one, and French of France when there is no Canadian French).

**Read of the format:** stems and their flags, `PFX`/`SFX` affixes with their conditions (including
cross-product ones), `NEEDAFFIX`, `FORBIDDENWORD`, `NOSUGGEST`, and the three ways flags are written
(one character, `FLAG long`, `FLAG num`). **Not read:** compound rules, `ICONV`/`OCONV`, `SET`
(other than UTF-8 files being read as UTF-8), and the format's own suggestion engine — suggestions
are MailCraft's. A line that cannot be read is skipped; a dictionary that cannot be read at all is
said in File › Options rather than crashing the window.

When no dictionary is found, **nothing is flagged** (rather than every word), and File › Options
says that none was found: a window that underlined a whole message would be lying about it.

### Canadian spellings

`colour`, `centre`, `neighbour`, `Montréal`, `Québec`, `français`, `aujourd’hui` and `rendez-vous`
are all words when the Canadian dictionaries are installed — that is a matter of which dictionary
the system has, not of this program; the Tests below say which of them are held to that.

### Your own words

*Add to Dictionary* (right click, or the dialog) keeps a word in your settings — never in the
system's dictionary files. Words are compared without regard to case and with either apostrophe
(`aujourd'hui` and `aujourd’hui` are one word), duplicates are refused, and File › Options lists
them with a × to take one out. The list is capped at 10,000 words of 64 characters.

## Grammar

Grammar is LanguageTool's: MailCraft sends the text to `POST /v2/check` and reads the answer. The
default address is `http://127.0.0.1:8081`, which has to be a server on this computer.

Running one:

```
# Arch
sudo pacman -S languagetool
languagetool-server --port 8081 --allow-origin

# or the standalone package, which brings its own Java
java -jar languagetool-server.jar --port 8081
```

Then File › Options › Spelling & Grammar › **Check grammar with a LanguageTool server**, and
*Test the connection*. English and French rules are both there (`en-CA`, `fr-CA` are asked for by
their own names).

**Only this computer, unless you say otherwise.** An address that is not `http://`/`https://`, or
that is not loopback, is refused before a single byte is sent — refused by the window, by the
engine and again by the network layer, each on its own account. A server on another computer is
allowed only by ticking *Allow a grammar server on another computer*; turning that off also turns
grammar checking off if its server was elsewhere, so nothing carries on being sent by an old yes.

Nothing is asked while you type: the text rests 250 ms, at most one request is on its way, and
two are never asked for within 800 ms of each other. An answer for a text the message no longer
has is thrown away, and so is a place that does not fit the text it is applied to.

## Privacy

- Spelling is entirely local: no dictionary is fetched, no word is sent anywhere.
- Grammar goes to the server you named, and to nothing else — by default a program on your own
  machine. It is never sent anywhere else, there is no telemetry, and the draft never appears in a
  log line, an error message or a crash report.
- What is sent is the body's text as plain text (paragraphs joined by line breaks), for that one
  request, with the language's code. Nothing of the headers, the recipients or the attachments.
- A server's answer is treated as hostile: capped at 1 MB, no redirect followed, offsets checked
  against the text sent, a place that does not fit is dropped rather than clamped. LanguageTool's
  offsets are UTF-16 code units and are converted to MailCraft's byte positions once, with tests
  (`crates/model/src/proof/grammar.rs`).

## Commands

Everything here is a command, so the window, a script, the CLI and MCP do the same thing:

| Command | What it does |
|---|---|
| `proof.check` | the spelling places of the body (or a range, or a text) |
| `proof.suggest` | what could stand in for a word |
| `proof.change` | puts a word in place of a place, as one step of the message's undo |
| `proof.addWord` / `proof.removeWord` | the personal dictionary |
| `proof.ignore` / `proof.ignoreRule` / `proof.forgetRules` | one place, or a grammar rule, left quiet |
| `proof.set` / `proof.status` | the settings, and what was read |
| `review.spelling` | starts, or goes on with, the walk through everything found |

```
mailcraft-cli run --sample --cmd proof.check
mailcraft-cli run --sample --cmd proof.addWord --params '{"word": "Hochelaga"}'
mailcraft-cli run --sample --cmd proof.set --params '{"checkGrammar": true}'
```

A command that fails changes nothing: a correction whose place no longer fits the message (because
it was edited meanwhile) is refused, and the message is left as it was.

## Tests

- `crates/model/src/proof/tests.rs` — a word being right when either language says so, mixed
  sentences, hyphens, addresses left alone, spans that always fit, the personal dictionary.
- `crates/model/src/proof/spell_tests.rs` — the format: affixes and their conditions, the three
  flag kinds, words that may not be suggested, and a property test over arbitrary Unicode.
- `crates/model/src/proof/grammar_tests.rs` — the answer: UTF-16 offsets with accents and emoji,
  places that do not fit, hostile and oversized answers.
- `crates/net/src/languagetool_tests.rs` — `POST /v2/check` against servers of the test's own:
  success, 500, a hang-up, garbage, an oversized answer, a slow server, nothing listening, an
  address on another computer refused.
- `crates/engine/src/cmd/proof_tests.rs` — through the commands: mixed language checking, a
  correction as one step of undo, the look of the corrected word kept, a stale place refused, the
  walk, and the settings.
- `apps/mailcraft/src/proofreading_tests.rs` — the host's half: a place comes back with the right
  bytes, an unreachable server says so and says nothing else.

## What is not here

- **No Language Server Protocol.** An LSP server (LTeX+, for instance) could be a fourth provider,
  and the provider seam is a command (`proof.check`, plus the host's `Services::proof_check`), so
  adding one would not disturb the window. It is not in this change because the HTTP server gives
  the same grammar rules without a second protocol and a second dependency.
- **No compound-word rules** of the Hunspell format (`COMPOUNDMIN`, `COMPOUNDRULE`…), which German
  dictionaries need more than English or French ones.
- **No grammar on the browser build**: it has no sockets of its own, so nothing is asked.
- Grammar checking of a *selection* only, no "check the whole mailbox".
