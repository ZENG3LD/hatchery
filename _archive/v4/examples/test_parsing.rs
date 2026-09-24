//! Test dependency parsing from a PRD file

use hatchery::prd;
use std::path::Path;

fn main() {
    let prd_path = Path::new("test-dependencies.md");

    println!("Parsing PRD: {}", prd_path.display());
    println!();

    match prd::parse_prd(prd_path) {
        Ok(tasks) => {
            println!("Found {} tasks:", tasks.len());
            println!();

            for task in &tasks {
                println!("Task prd-{}:", task.id);
                println!("  Description: {}", task.description);
                println!("  Dependencies: {:?}", task.dependencies);
                println!("  Skill hint: {:?}", task.skill_hint);
                println!("  Done: {}", task.done);
                println!();
            }

            // Verify dependency relationships
            println!("\nDependency Analysis:");
            println!("--------------------");

            // Track A tasks (no dependencies)
            println!("Track A (Core Engine) - No dependencies:");
            for task in tasks.iter().filter(|t| t.id <= 2) {
                println!("  prd-{}: deps={:?}", task.id, task.dependencies);
            }

            // Track B tasks (depend on prd-1 and prd-2)
            println!("\nTrack B (CLI) - Depends on prd-1 AND prd-2:");
            for task in tasks.iter().filter(|t| t.id >= 3 && t.id <= 5) {
                println!("  prd-{}: deps={:?}", task.id, task.dependencies);
            }

            // Track C tasks (depend on prd-1)
            println!("\nTrack C (Advanced) - Depends on prd-1:");
            for task in tasks.iter().filter(|t| t.id >= 6 && t.id <= 7) {
                println!("  prd-{}: deps={:?}", task.id, task.dependencies);
            }

            // Track D tasks (independent)
            println!("\nTrack D (Testing) - No dependencies:");
            for task in tasks.iter().filter(|t| t.id >= 8 && t.id <= 9) {
                println!("  prd-{}: deps={:?}", task.id, task.dependencies);
            }

            // Integration task (no explicit dependencies in header)
            println!("\nIntegration:");
            for task in tasks.iter().filter(|t| t.id >= 10) {
                println!("  prd-{}: deps={:?}", task.id, task.dependencies);
            }

            // Skill hints
            println!("\n\nSkill Hints:");
            println!("------------");
            for task in &tasks {
                if let Some(skill) = &task.skill_hint {
                    println!("  prd-{}: {} skill", task.id, skill);
                }
            }
        }
        Err(e) => {
            eprintln!("Error parsing PRD: {}", e);
            std::process::exit(1);
        }
    }
}
