//! Tests of [`Project`] as the port that puts the files of a run on disk: the paths
//! the three writers wrote at are the paths it is given, from the one root.

use std::collections::{BTreeMap, BTreeSet};

use whyshelf_core::app::shelf::{
    Creation, DeclarationText, DeclarationWrite, DocWrite, FileSync, FileWrite, Initialized,
    ShelfApp, ShelfWrite,
};
use whyshelf_core::domain::location::RelPath;
use whyshelf_core::domain::nonempty::NonEmptyVec;
use whyshelf_core::domain::shelf::build::{DropNames, FormEntries, RawValue, Shelf};
use whyshelf_core::domain::shelf::schema::{BookName, BookSchemaDecl, KeyPatternDecl, Schema};

use super::Project;
use super::init::Template;

fn project(dir: &tempfile::TempDir) -> Project {
    Project::new(dir.path(), Some("tester".to_owned()), None)
}

fn rel(text: &str) -> RelPath {
    text.parse().unwrap()
}

fn files(paths: &[&str]) -> NonEmptyVec<RelPath> {
    let mut paths = paths.iter().map(|p| rel(p));
    NonEmptyVec::new(paths.next().unwrap(), paths.collect())
}

/// The entries of a document of a book that declares nothing, as they are.
fn entries_of(key: &str) -> FormEntries {
    let decls = vec![BookSchemaDecl {
        name: "spec".to_owned(),
        dir: "docs".to_owned(),
        scope: Default::default(),
        key: KeyPatternDecl::Prefixed {
            prefix: "S".to_owned(),
        },
        description: None,
        fields: Vec::new(),
        links: Vec::new(),
    }];
    let schema = Schema::validate(decls, &BTreeSet::new()).unwrap();
    let entries = BTreeMap::from([("key".to_owned(), RawValue::Text(key.to_owned()))]);
    Shelf::form_entries(
        &schema,
        &BookName::new("spec").unwrap(),
        &entries,
        &DropNames::none(),
    )
}

#[tokio::test]
async fn the_files_the_three_writers_wrote_are_put_on_disk_through_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(&dir);
    assert_eq!(
        project
            .declaration_writer()
            .create(&DeclarationText::new("[books]\n"))
            .await
            .unwrap(),
        Creation::Created
    );
    assert_eq!(
        project
            .file_writer()
            .create(&rel("data/deep/table.csv"), b"a,b\n")
            .await
            .unwrap(),
        Creation::Created
    );
    assert_eq!(
        project
            .doc_writer()
            .create(&rel("docs/S-1.md"), &entries_of("S-1"), "Body.\n")
            .await
            .unwrap(),
        Creation::Created
    );
    // Each path is the one its writer wrote at, from the root of the project.
    project
        .file_syncer()
        .persist(&files(&[
            ".whyshelf/books.toml",
            "data/deep/table.csv",
            "docs/S-1.md",
        ]))
        .await
        .unwrap();
    assert_eq!(
        tokio::fs::read(dir.path().join("data/deep/table.csv"))
            .await
            .unwrap(),
        b"a,b\n"
    );
}

#[tokio::test]
async fn a_path_that_no_writer_wrote_cannot_be_put_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let project = project(&dir);
    let error = project
        .persist(&files(&["docs/nowhere.md"]))
        .await
        .unwrap_err();
    assert!(error.step.contains("nowhere.md"), "{}", error.step);
    assert_eq!(error.source.kind(), std::io::ErrorKind::NotFound);
}

#[tokio::test]
async fn a_run_of_init_through_the_project_puts_its_files_on_disk_and_each_is_whole() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ShelfApp::new(project(&dir));
    let Initialized {
        declaration,
        files: others,
        docs,
        ..
    } = app.init(Template::Sample.request().unwrap()).await.unwrap();
    let mut written: Vec<RelPath> = vec![declaration];
    written.extend(others);
    written.extend(docs.iter().map(|d| d.file.clone()));
    // The files `init` wrote are all there and may be put on disk again, by the
    // paths the run knew them by.
    let all = NonEmptyVec::from_vec(written).unwrap();
    app.ports().persist(&all).await.unwrap();
    for path in all.iter() {
        assert!(
            tokio::fs::metadata(dir.path().join(path.as_str()))
                .await
                .unwrap()
                .len()
                > 0,
            "{path}"
        );
    }
}
