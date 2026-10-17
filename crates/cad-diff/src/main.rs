//! `cad-diff`: runs the case catalogue through cad-geom and OCCT, loads the ASM reference, and
//! writes `research/VALIDATION_REPORT.md` and `research/evidence/diff/results.json`.

use cad_diff::cases::repo_root;
use cad_diff::report;

fn main() {
    // cad-geom bodies that are still `unimplemented!()` panic; the runner catches those and
    // records NOT_IMPLEMENTED, so keep the default hook from printing each one.
    std::panic::set_hook(Box::new(|_| {}));
    let root = repo_root();
    let out = match cad_diff::run(&root) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("cad-diff: {e}");
            std::process::exit(2);
        }
    };
    let _ = std::panic::take_hook();

    let dir = root.join("research").join("evidence").join("diff");
    let json = serde_json::to_string_pretty(&out).expect("results serialise");
    let md = report::render(&out, &out.catalogue);
    let write = |p: std::path::PathBuf, text: &str| {
        if let Err(e) = std::fs::create_dir_all(p.parent().expect("has parent"))
            .and_then(|_| std::fs::write(&p, text))
        {
            eprintln!("cad-diff: could not write {}: {e}", p.display());
            std::process::exit(1);
        }
        println!("wrote {}", p.display());
    };
    write(dir.join("results.json"), &json);
    write(root.join("research").join("VALIDATION_REPORT.md"), &md);

    let mut tally = std::collections::BTreeMap::<String, usize>::new();
    for c in &out.comparisons {
        *tally
            .entry(format!("{} vs {}: {}", c.a, c.b, c.outcome.label()))
            .or_default() += 1;
    }
    for (k, v) in tally {
        println!("{k} = {v}");
    }
    println!("ASM: {}", out.asm_status);
    for b in &out.bridge_fidelity {
        println!(
            "STEP bridge {}: {}",
            b.primitive,
            if b.identical {
                "identical".to_string()
            } else {
                b.differences.join("; ")
            }
        );
    }
}
