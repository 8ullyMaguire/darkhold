//! The baseline ladder: turn "is this engine good?" into a comparison with a floor.
//!
//! ```text
//! cargo run -p darkhold-taste --example ladder -- \
//!     --set eval/eval-set.json --challenger popularity
//! ```
//!
//! One input file, because the export already joined the readers, the judgements and every rung's
//! ranking on one split. The module ([`darkhold_taste::ladder`]) does the scoring and knows nothing
//! about JSON; this binary is the I/O.
//!
//! # What it refuses to do
//!
//! - **It does not score a refused rung as zero.** A rung the export could not compute arrives with
//!   a `refused` reason and is printed as `REFUSED`, because a missing table at the bottom of a
//!   table reads as "that engine is bad".
//! - **It does not exit 0 on a run that measured nothing.** A refusal, an unshipable challenger and
//!   a malformed file are three different non-zero exits, and the shell can tell them apart.
//! - **It does not invent the `random` rung's draws from the clock.** The seed is the reader's hash,
//!   so two runs over the same file print the same table. A fixture that changes between runs
//!   certifies nothing.
//! - **It does not accept a bar from the command line.** The threshold is pre-registered in the
//!   module (`Bar::proposed`) precisely so it cannot be tuned after the numbers are seen.

use std::collections::BTreeMap;
use std::process::ExitCode;

use darkhold_taste::eval::Judgement;
use darkhold_taste::ladder::{Bar, Case, LadderReport, Rung, K};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SetFile {
    #[serde(default)]
    catalogue_size: usize,
    #[serde(default)]
    cutoff: String,
    #[serde(default)]
    groups: BTreeMap<String, String>,
    /// The works a `random` rung may draw from. Absent means no random rung is derived.
    #[serde(default)]
    work_ids: Vec<String>,
    #[serde(default)]
    readers: Vec<ReaderFile>,
}

#[derive(Debug, Deserialize)]
struct ReaderFile {
    reader: String,
    #[serde(default)]
    judged: Vec<JudgementJson>,
    #[serde(default)]
    rungs: Vec<RungFile>,
}

#[derive(Debug, Deserialize)]
struct JudgementJson {
    work_id: String,
    #[serde(default = "one")]
    grade: u32,
}

#[derive(Debug, Deserialize)]
struct RungFile {
    name: String,
    #[serde(default)]
    ranking: Vec<String>,
    #[serde(default)]
    refused: Option<String>,
}

fn one() -> u32 {
    1
}

fn read(path: &str) -> Result<SetFile, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))
}

/// FNV-1a over the reader id: a seed that is stable across runs and processes.
///
/// `DefaultHasher` is deliberately not used — it is randomised per process, which would make the
/// `random` rung different on every run and the fixture untestable.
fn seed_of(reader: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in reader.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// `k` distinct works drawn from `pool`, deterministically, by a xorshift walk.
///
/// The draw is without replacement: a rung that repeats a work wastes slots and `precision_at`
/// would charge it for the repeat, which is a real cost but not the one being measured.
fn draw(pool: &[String], k: usize, seed: u64) -> Vec<String> {
    if pool.is_empty() {
        return Vec::new();
    }
    let mut state = seed | 1;
    let mut taken: Vec<usize> = Vec::with_capacity(k.min(pool.len()));
    while taken.len() < k.min(pool.len()) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let index = (state % pool.len() as u64) as usize;
        if !taken.contains(&index) {
            taken.push(index);
        }
    }
    taken.into_iter().map(|index| pool[index].clone()).collect()
}

fn main() -> ExitCode {
    let mut set_path = None;
    let mut challenger = String::from("popularity");
    let mut out_path: Option<String> = None;

    let usage = "usage: ladder --set SET.json [--challenger NAME] [--out REPORT.json]";
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--set" => set_path = args.next(),
            "--challenger" => {
                if let Some(name) = args.next() {
                    challenger = name;
                }
            }
            "--out" => out_path = args.next(),
            "--help" | "-h" => {
                println!("{usage}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::from(2);
            }
        }
    }

    let Some(set_path) = set_path else {
        eprintln!("{usage}");
        return ExitCode::from(2);
    };

    let set = match read(&set_path) {
        Ok(set) => set,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };

    let groups: std::collections::HashMap<String, String> = set.groups.into_iter().collect();
    let mut cases = Vec::with_capacity(set.readers.len());
    for reader in set.readers {
        let mut rungs: Vec<Rung> = reader
            .rungs
            .into_iter()
            .map(|rung| Rung {
                name: rung.name,
                ranking: rung.ranking,
                refused: rung.refused,
            })
            .collect();

        // The random rung is derived here rather than exported: it needs the catalogue and a seed,
        // and the export's job is to state facts, not to make draws.
        if !rungs.iter().any(|rung| rung.name == "random") {
            let ranking = if set.work_ids.is_empty() {
                Vec::new()
            } else {
                draw(&set.work_ids, K, seed_of(&reader.reader))
            };
            rungs.push(Rung {
                name: "random".to_string(),
                ranking,
                refused: if set.work_ids.is_empty() {
                    Some("the set carries no work_ids, so no draw is possible".to_string())
                } else {
                    None
                },
            });
        }

        cases.push(Case {
            reader: reader.reader,
            judged: reader
                .judged
                .into_iter()
                .map(|judgement| Judgement {
                    work_id: judgement.work_id,
                    grade: judgement.grade,
                })
                .collect(),
            rungs,
        });
    }
    let readers = cases.len();

    let report = LadderReport::build(&cases, set.catalogue_size, &groups, Bar::proposed());

    if !set.cutoff.is_empty() {
        println!("cutoff {}", set.cutoff);
    }
    print!("{}", report.to_text(&challenger));

    let verdict = report.verdict(&challenger);
    if let Some(path) = &out_path {
        let body = serde_json::json!({
            "k": report.k,
            "cutoff": set.cutoff,
            "catalogue_size": report.catalogue_size,
            "readers": report.readers,
            "scored": report.scored,
            "bar": {
                "popularity_multiple": report.bar.popularity_multiple,
                "min_catalogue_share": report.bar.min_catalogue_share,
                "max_group_share": report.bar.max_group_share,
            },
            "rungs": report.rungs.iter().map(|rung| serde_json::json!({
                "name": rung.name,
                "refused": rung.refused,
                "scored": rung.scored,
                "precision_at_k": rung.precision_at_k,
                "recall_at_k": rung.recall_at_k,
                "f1_at_k": rung.f1_at_k,
                "map_at_k": rung.map_at_k,
                "ndcg_at_10": rung.ndcg_at_10,
                "distinct_works": rung.distinct_works,
                "catalogue_share": rung.catalogue_share,
                "largest_group_share": rung.largest_group_share,
            })).collect::<Vec<_>>(),
            "verdict": {
                "challenger": verdict.challenger,
                "f1_ratio": verdict.f1_ratio,
                "shipped": verdict.shipped,
                "reason": verdict.reason,
            },
        });
        match std::fs::write(path, format!("{body:#}\n")) {
            Ok(()) => println!("\nreport written to {path}"),
            Err(error) => eprintln!("{path}: {error}"),
        }
    }

    // Three exits, and each one means something a shell can act on.
    if readers == 0 {
        eprintln!("\nno reader in the set: this run measured nothing and is not a pass.");
        return ExitCode::from(1);
    }
    if report.rungs.iter().any(|rung| rung.refused.is_some()) {
        eprintln!(
            "\n{} rung(s) refused: the ladder is incomplete and no claim can be made from it.",
            report
                .rungs
                .iter()
                .filter(|rung| rung.refused.is_some())
                .count()
        );
        return ExitCode::from(1);
    }
    if !verdict.shipped {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
