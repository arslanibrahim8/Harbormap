//! `package.json` entries: each `bin` command, `main`, and `scripts.start` (Learn spec
//! §5.1). Fields: <https://docs.npmjs.com/cli/v11/configuring-npm/package-json>.

use hm_core::ids::file_id;

use super::{Derived, Inputs, Route, is_known, route};
use crate::manifest::NpmManifest;
use crate::ts::tsconfig::join_norm;

/// The programs whose first file argument is the script that `start` runs.
const RUNNERS: &[&str] = &["node", "tsx", "ts-node", "bun"];

/// Words a runner takes before the file (`tsx watch x.ts`, `bun run x.ts`).
const RUNNER_SUBCOMMANDS: &[&str] = &["run", "watch"];

/// Flags whose value is the next word (`-r ./tracing.js`), not the script.
const FLAGS_WITH_VALUE: &[&str] = &["-r", "--require", "--import", "--loader", "--env-file"];

/// Flags that run code given on the command line: there is no script file.
const EVAL_FLAGS: &[&str] = &["-e", "--eval", "-p", "--print"];

/// One `cli` route per `bin`, a `main` route for `main` and one for `scripts.start`.
pub(crate) fn routes(inputs: &Inputs, out: &mut Derived) {
    for m in &inputs.manifests.npm {
        let entry = |kind: &str, method: &str, pattern: &str, line: u32, rule: &str| Route {
            prov: "config".to_string(),
            ..route(&m.root, kind, method, pattern, &m.path, line, rule)
        };
        let target = |path: &str| {
            join_norm(&m.root, path)
                .filter(|rel| is_known(inputs.project, rel))
                .map(|rel| file_id(&rel))
        };

        for (name, path, line) in &m.bin {
            out.routes.push(Route {
                handler_id: target(path),
                ..entry("cli", "BIN", name, *line, "npm.bin")
            });
        }
        if let Some((path, line)) = &m.main {
            out.routes.push(Route {
                handler_id: target(path),
                ..entry("main", "MAIN", path, *line, "npm.main")
            });
        }
        if let Some((_, command, line)) = start_script(m) {
            out.routes.push(Route {
                handler_id: started_file(command).and_then(target),
                detail: Some(serde_json::json!({ "command": command }).to_string()),
                ..entry("main", "START", "start", *line, "npm.start")
            });
        }
    }
}

fn start_script(m: &NpmManifest) -> Option<&(String, String, u32)> {
    m.scripts.iter().find(|(name, _, _)| name == "start")
}

/// The file `node|tsx|ts-node|bun <file>` runs, as written: the first argument after the
/// runner that is not a flag, a flag's value or a subcommand. Leading `NAME=value`
/// assignments are skipped; a command that evaluates code (`node -e …`) runs no file.
fn started_file(command: &str) -> Option<&str> {
    let mut words = command
        .split_whitespace()
        .skip_while(|w| w.contains('=') && !w.starts_with('-'));
    let runner = words.next()?;
    if !RUNNERS.contains(&runner) {
        return None;
    }
    while let Some(word) = words.next() {
        if word.starts_with('-') {
            // `--require=./x.js` carries its value; `--require ./x.js` takes the next word.
            let flag = word.split_once('=').map_or(word, |(flag, _)| flag);
            if EVAL_FLAGS.contains(&flag) {
                return None;
            }
            if flag == word && FLAGS_WITH_VALUE.contains(&flag) {
                words.next();
            }
        } else if !RUNNER_SUBCOMMANDS.contains(&word) {
            return Some(word);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::started_file;

    #[test]
    fn the_started_file_follows_the_runner() {
        assert_eq!(started_file("node server.js"), Some("server.js"));
        assert_eq!(
            started_file("NODE_ENV=production node --enable-source-maps dist/main.js"),
            Some("dist/main.js")
        );
        assert_eq!(started_file("tsx watch src/index.ts"), Some("src/index.ts"));
        assert_eq!(started_file("bun run src/index.ts"), Some("src/index.ts"));
        assert_eq!(started_file("next start"), None);
        assert_eq!(started_file("node"), None);
        // The value of a flag is not the script.
        assert_eq!(
            started_file("node -r ./tracing.js server.js"),
            Some("server.js")
        );
        assert_eq!(
            started_file("node --import tsx --env-file .env src/main.ts"),
            Some("src/main.ts")
        );
        assert_eq!(
            started_file("node --require=./tracing.js --loader ts-node/esm app.ts"),
            Some("app.ts")
        );
        assert_eq!(started_file("node -e \"require('./x')\""), None);
        assert_eq!(started_file("node --eval=1"), None);
        assert_eq!(started_file("node -p 1"), None);
    }
}
