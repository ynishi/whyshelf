//! New document files: [`ScaffoldPort`] writes a template, never over a file.
//!
//! The application knows a [`Template`] as a value (a type and a title, the
//! grounds, the Spec); this module knows what a file looks like and renders it,
//! so the file format lives in one crate. A template asks for what only a person
//! can say with an empty value, so a fresh file often does not parse yet: it is
//! registered and shown as invalid until it is filled in, which is what a person
//! expects of a form.
//!
//! The file is created with the operating system's atomic "create, but only if
//! it does not exist" (see [`crate::blocking::create_new_file`]), so the tool
//! never rewrites a file somebody has written, not even by a race.

use std::fmt::Write;
use std::path::PathBuf;

use whyshelf_core::app::ports::{CreateOutcome, ScaffoldPort, Template};
use whyshelf_core::domain::finding::FindingType;
use whyshelf_core::domain::location::RelPath;

use crate::blocking::{self, BlockingError};

/// Why a file could not be created.
#[derive(Debug, thiserror::Error)]
#[error("cannot create the file {path}")]
pub struct ScaffoldError {
    path: RelPath,
    #[source]
    source: BlockingError,
}

/// A text as a double-quoted YAML scalar (JSON strings are YAML strings).
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".to_owned())
}

fn finding_keys(finding_type: FindingType) -> &'static str {
    match finding_type {
        FindingType::MeasuredData => {
            "measured_at: \"\"   # RFC 3339, for example 2026-01-31T09:30:00Z\n\
             revision: \"\"      # the revision that was measured\n\
             environment: \"\"\n"
        }
        FindingType::UsageObservation => {
            "context: \"\"        # the situation: where, which steps\n\
             reproducibility: reproducible   # reproducible | not-reproducible\n"
        }
        FindingType::ExternalSource => {
            "source: \"\"         # a URI, or a citation in free text\n\
             version: \"\"        # the version of the source this applies to\n\
             maturity: emerging   # experimental | emerging | established | standard\n"
        }
        FindingType::CodeObservation => {
            "ref: \"\"            # path:line, for example src/parser.rs:42\n\
             revision: \"\"      # the revision that was looked at\n\
             method: read        # read | executed\n"
        }
        FindingType::DomainConstraint => {
            "authority: \"\"      # who decided\n\
             scope: \"\"          # what it applies to\n\
             review_condition: \"\"   # when to look at it again\n"
        }
    }
}

/// The text of a new file.
pub fn render(template: &Template) -> String {
    let mut out = String::from("---\n");
    match template {
        Template::Finding {
            finding_type,
            title,
        } => {
            let _ = writeln!(out, "finding_type: {}", finding_type.name());
            let _ = writeln!(out, "title: {}", quoted(title.as_str()));
            out.push_str("evidence: measured   # measured | theoretical | estimated\n");
            out.push_str(finding_keys(*finding_type));
            out.push_str("---\n\n<!-- Write what was seen, and how it is known. -->\n");
        }
        Template::Spec { grounds } => {
            out.push_str("grounds:\n");
            for ground in grounds.iter() {
                let _ = writeln!(out, "  - {}", quoted(&ground.to_string()));
            }
            out.push_str(
                "accept:\n  pins: []   # - id: PIN-NAME\n            #   contract: \"the sentence the test guarantees\"\n  \
                 hyp: []    # - id: HYP-NAME\n            #   metric: \"what is measured\"\n            #   target: \"how it should move\"\n",
            );
            out.push_str("---\n\n## Contract\n\n## Design Notes\n");
        }
        Template::Design { spec } => {
            let _ = writeln!(out, "spec: {}", quoted(&spec.to_string()));
            out.push_str(
                "crux:\n  question: \"\"\n  options: []\n  # - id: opt-name\n  #   summary: \"what the option is\"\n  \
                 #   chosen: true\n  #   reason: \"why it was chosen\"\n  # - id: opt-other\n  #   summary: \"what the option is\"\n  \
                 #   rejected: \"why it was not chosen\"\n",
            );
            out.push_str(
                "items: []\n# - number: 1\n#   summary: \"what this item does\"\n#   pins: [PIN-NAME]\n",
            );
            out.push_str("---\n\n## Design Notes\n");
        }
    }
    out
}

/// Creates new files in the working tree.
#[derive(Debug, Clone)]
pub struct FsScaffold {
    root: PathBuf,
}

impl FsScaffold {
    /// Files are created under this project root.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl ScaffoldPort for FsScaffold {
    type Error = ScaffoldError;

    async fn create_new(
        &self,
        path: &RelPath,
        template: &Template,
    ) -> Result<CreateOutcome, ScaffoldError> {
        let created =
            blocking::create_new_file(self.root.join(path.as_str()), render(template).into_bytes())
                .await
                .map_err(|source| ScaffoldError {
                    path: path.clone(),
                    source,
                })?;
        Ok(if created {
            CreateOutcome::Created
        } else {
            CreateOutcome::AlreadyExists
        })
    }

    async fn remove_created(
        &self,
        path: &RelPath,
        template: &Template,
    ) -> Result<bool, ScaffoldError> {
        blocking::remove_file_if_content(
            self.root.join(path.as_str()),
            render(template).into_bytes(),
        )
        .await
        .map_err(|source| ScaffoldError {
            path: path.clone(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{markdown, parse};
    use whyshelf_core::app::raw::RawDoc;
    use whyshelf_core::domain::nonempty::{NonBlank, NonEmptyVec};
    use whyshelf_core::domain::reference::Ref;

    fn finding_template(finding_type: FindingType) -> Template {
        Template::Finding {
            finding_type,
            title: NonBlank::new("Login \"feels\" slow").unwrap(),
        }
    }

    #[test]
    fn every_finding_template_is_yaml_that_asks_for_what_is_missing() {
        for finding_type in FindingType::ALL {
            let text = render(&finding_template(finding_type));
            let reasons = parse::parse(&markdown::normalize(&text))
                .expect_err("a template is not a finished document");
            // The title was given and is quoted so that it survives.
            assert!(
                reasons.iter().all(|r| !matches!(
                    r,
                    whyshelf_core::domain::state::UnparsedReason::MalformedFrontmatter { .. }
                )),
                "{finding_type:?}: {reasons:?}"
            );
            assert!(text.contains("title: \"Login \\\"feels\\\" slow\""));
        }
    }

    #[test]
    fn a_filled_in_finding_template_parses() {
        let text = render(&finding_template(FindingType::UsageObservation))
            .replace("context: \"\"", "context: \"cold start\"");
        let doc = parse::parse(&markdown::normalize(&text)).unwrap();
        assert!(matches!(doc, RawDoc::Finding(_)));
    }

    #[test]
    fn spec_and_design_templates_carry_their_upstream() {
        let grounds = NonEmptyVec::new("finding/slow-login".parse::<Ref>().unwrap(), Vec::new());
        let spec = render(&Template::Spec { grounds });
        assert!(spec.contains("  - \"finding/slow-login\""));
        assert!(spec.contains("## Contract"));
        let filled = spec.replace("## Contract\n", "## Contract\nThe session is reused.\n");
        assert!(matches!(
            parse::parse(&markdown::normalize(&filled)).unwrap(),
            RawDoc::Spec(_)
        ));

        let design = render(&Template::Design {
            spec: "spec/session-reuse".parse().unwrap(),
        });
        assert!(design.contains("spec: \"spec/session-reuse\""));
        assert!(parse::parse(&markdown::normalize(&design)).is_err());
    }

    #[tokio::test]
    async fn a_file_is_created_once_and_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let scaffold = FsScaffold::new(dir.path().to_path_buf());
        let path: RelPath = "docs/finding/slow-login.md".parse().unwrap();
        let template = finding_template(FindingType::UsageObservation);
        assert_eq!(
            scaffold.create_new(&path, &template).await.unwrap(),
            CreateOutcome::Created
        );
        let written = tokio::fs::read_to_string(dir.path().join(path.as_str()))
            .await
            .unwrap();
        assert_eq!(written, render(&template));

        tokio::fs::write(dir.path().join(path.as_str()), "mine\n")
            .await
            .unwrap();
        assert_eq!(
            scaffold.create_new(&path, &template).await.unwrap(),
            CreateOutcome::AlreadyExists
        );
        let after = tokio::fs::read_to_string(dir.path().join(path.as_str()))
            .await
            .unwrap();
        assert_eq!(after, "mine\n");
    }

    #[tokio::test]
    async fn only_a_file_still_exactly_the_template_is_taken_away() {
        let dir = tempfile::tempdir().unwrap();
        let scaffold = FsScaffold::new(dir.path().to_path_buf());
        let path: RelPath = "docs/finding/slow-login.md".parse().unwrap();
        let on_disk = dir.path().join(path.as_str());
        let template = finding_template(FindingType::UsageObservation);

        scaffold.create_new(&path, &template).await.unwrap();
        assert!(scaffold.remove_created(&path, &template).await.unwrap());
        assert!(!on_disk.exists());
        // Nothing there: nothing to take away.
        assert!(!scaffold.remove_created(&path, &template).await.unwrap());

        // Somebody filled it in since: it is theirs now.
        scaffold.create_new(&path, &template).await.unwrap();
        tokio::fs::write(&on_disk, "filled in\n").await.unwrap();
        assert!(!scaffold.remove_created(&path, &template).await.unwrap());
        assert_eq!(
            tokio::fs::read_to_string(&on_disk).await.unwrap(),
            "filled in\n"
        );
    }
}
