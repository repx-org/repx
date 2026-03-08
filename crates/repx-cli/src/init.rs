use colored::Colorize;
use std::path::Path;
use std::process::Command;

struct TemplateFile {
    path: &'static str,
    content: &'static str,
}

const TEMPLATES: &[TemplateFile] = &[
    TemplateFile {
        path: ".envrc",
        content: include_str!("../templates/init/.envrc"),
    },
    TemplateFile {
        path: ".gitignore",
        content: include_str!("../templates/init/.gitignore"),
    },
    TemplateFile {
        path: "flake.nix",
        content: include_str!("../templates/init/flake.nix"),
    },
    TemplateFile {
        path: "nix/lab.nix",
        content: include_str!("../templates/init/nix/lab.nix"),
    },
    TemplateFile {
        path: "nix/run.nix",
        content: include_str!("../templates/init/nix/run.nix"),
    },
    TemplateFile {
        path: "nix/pipeline.nix",
        content: include_str!("../templates/init/nix/pipeline.nix"),
    },
    TemplateFile {
        path: "nix/stage-hello.nix",
        content: include_str!("../templates/init/nix/stage-hello.nix"),
    },
];

pub fn handle_init(path: &Path, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }

    if path.join("flake.nix").exists() {
        return Err(format!(
            "flake.nix already exists in {}",
            path.canonicalize()?.display()
        )
        .into());
    }

    std::fs::create_dir_all(path.join("nix"))?;

    for template in TEMPLATES {
        let content = template.content.replace("{{name}}", name);
        let file_path = path.join(template.path);
        std::fs::write(&file_path, content)?;
    }

    let in_git_repo = Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .current_dir(path)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !in_git_repo {
        let git_init = Command::new("git").arg("init").current_dir(path).output();

        match git_init {
            Ok(output) if output.status.success() => {}
            _ => {
                eprintln!("{}", "warning: failed to run `git init`; skipping".yellow());
            }
        }
    }

    let display_path = if path == Path::new(".") {
        std::env::current_dir()?
    } else {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    };

    eprintln!(
        "{} repx experiment '{}' in {}",
        "Created".green().bold(),
        name,
        display_path.display()
    );
    eprintln!();
    eprintln!("  To get started:");
    if path != Path::new(".") {
        eprintln!("    cd {}", path.display());
    }
    eprintln!("    nix build");
    eprintln!("    repx run");
    eprintln!();

    Ok(())
}
