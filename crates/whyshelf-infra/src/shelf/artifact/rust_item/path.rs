//! The names of the space as paths: the one reader of their grammar ([`RustPath`]),
//! the one place a name is made from its segments ([`RustPath::as_name`]), and what
//! the adapter says of a move of a prefix of them ([`renamed`]).
//!
//! | what | what it does |
//! |---|---|
//! | [`RustPath::parse`] | the one reader of the grammar of the names |
//! | [`RustPath::as_name`] | the one place the segments of a path are joined with `::` |
//! | [`RustPath::shared_end`] | the one place the end of a name is read: how many segments two paths end with alike |
//! | [`RustPath::moves_between`] | every move of a prefix that leads from a name to a candidate, the widest first: the leading parts of the two before an end they share, down to the whole of both, each kept only when [`renamed`] maps the name to exactly the candidate |
//! | [`renamed`] | which names are under a prefix, and what each becomes under the new one |
//!
//! This module reads no file and depends on no sibling.

use std::collections::BTreeSet;

use whyshelf_core::domain::error::ValueError;
use whyshelf_core::domain::nonempty::NonBlank;
use whyshelf_core::domain::shelf::artifact::{
    ArtifactName, NamePrefix, PrefixMove, Renamed, SpacePrefixes, SpaceRenamed,
};

use super::super::{built, unbuildable};

/// A name of the space, read: the crate and the segments after it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct RustPath {
    pub(super) krate: String,
    pub(super) rest: Vec<String>,
}

impl RustPath {
    /// The one reader of the grammar of the names.
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let segments: Vec<&str> = text.split("::").collect();
        let krate = segments[0];
        if krate.contains('-') && segment_problem(&krate.replace('-', "_")).is_none() {
            return Err(format!(
                "no Rust path: a crate is named as rustdoc writes it, with `_` for `-` (`{}`)",
                krate.replace('-', "_")
            ));
        }
        for segment in &segments {
            if let Some(why) = segment_problem(segment) {
                return Err(format!("no Rust path (`crate::module::Item`): {why}"));
            }
        }
        Ok(Self {
            krate: krate.to_owned(),
            rest: segments[1..].iter().map(|s| (*s).to_owned()).collect(),
        })
    }

    /// The segments, the crate first: what a prefix is compared on.
    fn segments(&self) -> Vec<&str> {
        std::iter::once(self.krate.as_str())
            .chain(self.rest.iter().map(String::as_str))
            .collect()
    }

    /// The text of the path, the segments joined by `::`: the one place a name of the
    /// space is made from its segments, the reverse of [`RustPath::parse`].
    pub(super) fn as_name(&self) -> String {
        self.segments().join("::")
    }

    /// The path of these segments, the crate first, when each is a segment
    /// [`RustPath::parse`] reads (the same rule, held in one place); none when there is
    /// none or one is not.
    pub(super) fn of(segments: &[String]) -> Option<Self> {
        let (krate, rest) = segments.split_first()?;
        segments
            .iter()
            .all(|segment| segment_problem(segment).is_none())
            .then(|| Self {
                krate: krate.clone(),
                rest: rest.to_vec(),
            })
    }

    /// This path with `member` after it, when that is a path; none when it is not
    /// (`member` is no identifier).
    pub(super) fn with(&self, member: &str) -> Option<Self> {
        let mut segments: Vec<String> = self.segments().into_iter().map(str::to_owned).collect();
        segments.push(member.to_owned());
        Self::of(&segments)
    }

    /// The path of the type a name would be a member of, and the member: the name
    /// without its last segment and that segment, when what is left names more than
    /// a crate (a crate has no members); none for the path of a crate or of an item
    /// directly in one.
    pub(super) fn owner(&self) -> Option<(Self, &str)> {
        let (member, owner) = self.rest.split_last()?;
        if owner.is_empty() {
            return None;
        }
        Some((
            Self {
                krate: self.krate.clone(),
                rest: owner.to_vec(),
            },
            member.as_str(),
        ))
    }

    /// How many segments this path and `other` end with alike, counted from the last:
    /// the one place the end of a name is read.
    pub(super) fn shared_end(&self, other: &Self) -> usize {
        self.last_first()
            .zip(other.last_first())
            .take_while(|(a, b)| a == b)
            .count()
    }

    /// The segments, the last first.
    fn last_first(&self) -> impl Iterator<Item = &String> {
        self.rest.iter().rev().chain(std::iter::once(&self.krate))
    }

    /// The moves of a prefix that lead from this name to `candidate`, the widest
    /// first (the fewest segments in the prefix). A move is a leading part of each name,
    /// or the whole of it, with the same segments after it in both: the segments left
    /// out are an end the two names share, so the count of them goes from the most that
    /// leaves a segment in each part (all of the shared end, or one fewer than the
    /// shorter name has) down to none, which is the whole of both, the name moved as
    /// itself. That last move is there whatever the two names share, so names that
    /// share no end have it alone. A module that keeps its name under another parent is
    /// so moved by its parent (the widest), by the module itself, and by each name in
    /// it; and a name that ends with all of the other (`old::Foo` and `x::old::Foo`) by
    /// `old` to `x::old` and by the whole name. None at all for one name.
    ///
    /// A move is kept only when the renamed core ([`renamed`]) maps this name to
    /// exactly the candidate under it, so that what is suggested is what `judge moved
    /// --prefix` would do, and the moves are all of those that do.
    pub(super) fn moves_between(&self, candidate: &Self) -> Vec<PrefixMove> {
        if self == candidate {
            return Vec::new();
        }
        let most = self
            .shared_end(candidate)
            .min(self.segments().len() - 1)
            .min(candidate.segments().len() - 1);
        (0..=most)
            .rev()
            .filter_map(|outside| self.move_leaving(candidate, outside))
            .collect()
    }

    /// The move of the prefix that is the leading part of each name, `outside`
    /// segments from the last left out, when the renamed core maps this name to exactly
    /// `candidate` under it.
    fn move_leaving(&self, candidate: &Self, outside: usize) -> Option<PrefixMove> {
        let leading = |path: &Self| -> Option<NamePrefix> {
            let segments = path.segments();
            let kept = segments.get(..segments.len().checked_sub(outside)?)?;
            let part = Self::of(&kept.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>())?;
            NamePrefix::new(&part.as_name()).ok()
        };
        let of = PrefixMove::new(leading(self)?, leading(candidate)?).ok()?;
        let gone = ArtifactName::new(&self.as_name()).ok()?;
        let (_, answers) = renamed(&BTreeSet::from([gone.clone()]), &of).ok()?;
        match answers.get(&gone) {
            Some(Renamed::Under(name)) if name.as_str() == candidate.as_name() => Some(of),
            _ => None,
        }
    }
}

/// What this adapter says of a move of a prefix, for the `names` of its space asked
/// about: a prefix is a Rust path ([`RustPath::parse`]) of one segment or more, and a
/// name is under it when its segments start with all of the prefix's, the same name
/// under `to` being `to`'s segments and the rest of the name's. `from` that is no Rust
/// path (a directory, say) is no prefix of the space's names; a `to` that is none
/// makes each name under `from` unbuildable. A name that is no Rust path is not under
/// it. Nothing is looked for in the text of a name.
pub(super) fn renamed(
    names: &BTreeSet<ArtifactName>,
    of: &PrefixMove,
) -> Result<(SpacePrefixes, SpaceRenamed), ValueError> {
    let from = match RustPath::parse(of.from().as_str()) {
        Ok(from) => from,
        Err(why) => {
            let why = NonBlank::new(format!(
                "`{}` is no prefix of a Rust path: {why}",
                of.from()
            ))?;
            return Ok((SpacePrefixes::NotUnder(why), SpaceRenamed::new()));
        }
    };
    let to = RustPath::parse(of.to().as_str());
    let from = from.segments();
    let mut answers = SpaceRenamed::new();
    for name in names {
        let answer = match RustPath::parse(name.as_str()) {
            Err(_) => Renamed::Outside,
            Ok(path) => {
                let segments = path.segments();
                match (segments.strip_prefix(from.as_slice()), &to) {
                    (None, _) => Renamed::Outside,
                    (Some(_), Err(why)) => {
                        unbuildable(format!("`{}` is no prefix of a Rust path: {why}", of.to()))?
                    }
                    (Some(rest), Ok(to)) => {
                        let moved = RustPath {
                            krate: to.krate.clone(),
                            rest: to
                                .rest
                                .iter()
                                .cloned()
                                .chain(rest.iter().map(|segment| (*segment).to_owned()))
                                .collect(),
                        };
                        built(&moved.as_name())?
                    }
                }
            }
        };
        answers.insert(name.clone(), answer);
    }
    Ok((SpacePrefixes::Answered, answers))
}

/// Why a segment of a path is no identifier, if it is none.
fn segment_problem(segment: &str) -> Option<String> {
    if segment.is_empty() {
        return Some("a segment between `::` is empty".to_owned());
    }
    if ["crate", "self", "super", "Self"].contains(&segment) {
        return Some(format!(
            "`{segment}` is relative: a path starts from the name of a crate"
        ));
    }
    if segment.starts_with("r#") {
        return Some(format!(
            "`{segment}` is a raw identifier, which is no name of the space"
        ));
    }
    let first = segment
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_');
    let rest = segment.chars().all(|c| c.is_alphanumeric() || c == '_');
    if !first || !rest || segment == "_" {
        return Some(format!("`{segment}` is no identifier"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> RustPath {
        RustPath::parse(text).unwrap()
    }

    fn name(text: &str) -> ArtifactName {
        ArtifactName::new(text).unwrap()
    }

    fn mv(from: &str, to: &str) -> PrefixMove {
        PrefixMove::new(NamePrefix::new(from).unwrap(), NamePrefix::new(to).unwrap()).unwrap()
    }

    #[test]
    fn the_end_two_paths_share_is_counted_from_the_last_segment() {
        let shared = |a: &str, b: &str| path(a).shared_end(&path(b));
        assert_eq!(shared("old::m::Foo", "new::m::Foo"), 2);
        assert_eq!(shared("old::Foo", "new::sub::Foo"), 1);
        assert_eq!(shared("old::Foo", "old::Foo"), 2);
        assert_eq!(shared("old::Foo", "new::Bar"), 0);
        // A segment is compared as written, whole.
        assert_eq!(shared("old::Foo", "new::OldFoo"), 0);
        // A crate is a segment too.
        assert_eq!(shared("a::b", "x::a::b"), 2);
    }

    #[test]
    fn the_moves_between_two_names_are_the_leading_parts_before_an_end_they_share_and_the_whole_of_both()
     {
        let between = |gone: &str, candidate: &str| path(gone).moves_between(&path(candidate));
        // One shared end segment: the part before it, then the whole of both.
        assert_eq!(
            between("old::Foo", "new::Foo"),
            [mv("old", "new"), mv("old::Foo", "new::Foo")]
        );
        assert_eq!(
            between("old::Foo", "new::sub::Foo"),
            [mv("old", "new::sub"), mv("old::Foo", "new::sub::Foo")]
        );
        assert_eq!(
            between("app::old::Foo::new", "app::fresh::Foo::new"),
            [
                mv("app::old", "app::fresh"),
                mv("app::old::Foo", "app::fresh::Foo"),
                mv("app::old::Foo::new", "app::fresh::Foo::new")
            ]
        );
        // Nothing in common at the end: the whole of both alone.
        assert_eq!(
            between("old::Foo", "new::Bar"),
            [mv("old::Foo", "new::Bar")]
        );
    }

    #[test]
    fn a_module_that_keeps_its_name_under_another_parent_is_moved_by_the_parent_the_module_and_the_name()
     {
        let between = |gone: &str, candidate: &str| path(gone).moves_between(&path(candidate));
        // The widest first: the parent, the module with it, then the name itself.
        assert_eq!(
            between("demo::app::x::Foo", "demo::domain::x::Foo"),
            [
                mv("demo::app", "demo::domain"),
                mv("demo::app::x", "demo::domain::x"),
                mv("demo::app::x::Foo", "demo::domain::x::Foo")
            ]
        );
        // The module itself is moved by the whole of its name.
        assert_eq!(
            between("demo::app::x", "demo::domain::x"),
            [
                mv("demo::app", "demo::domain"),
                mv("demo::app::x", "demo::domain::x")
            ]
        );
        assert_eq!(
            between("old::m::Foo", "new::sub::m::Foo"),
            [
                mv("old", "new::sub"),
                mv("old::m", "new::sub::m"),
                mv("old::m::Foo", "new::sub::m::Foo")
            ]
        );
        // Three shared end segments: each one segment narrower, to the whole.
        assert_eq!(
            between("a::p::q::r", "b::p::q::r"),
            [
                mv("a", "b"),
                mv("a::p", "b::p"),
                mv("a::p::q", "b::p::q"),
                mv("a::p::q::r", "b::p::q::r")
            ]
        );
    }

    #[test]
    fn a_name_that_ends_with_all_of_the_other_is_moved_by_the_part_before_its_end_and_by_the_whole()
    {
        let between = |gone: &str, candidate: &str| path(gone).moves_between(&path(candidate));
        // The split that leaves nothing outside is the whole of both; the one before it
        // has a segment on each side.
        assert_eq!(
            between("old::Foo", "x::old::Foo"),
            [mv("old", "x::old"), mv("old::Foo", "x::old::Foo")]
        );
        assert_eq!(
            between("x::old::Foo", "old::Foo"),
            [mv("x::old", "old"), mv("x::old::Foo", "old::Foo")]
        );
        assert_eq!(
            between("app::parser::Token", "parser::Token"),
            [
                mv("app::parser", "parser"),
                mv("app::parser::Token", "parser::Token")
            ]
        );
        assert_eq!(
            between("old::m::Foo", "x::old::m::Foo"),
            [
                mv("old", "x::old"),
                mv("old::m", "x::old::m"),
                mv("old::m::Foo", "x::old::m::Foo")
            ]
        );
    }

    #[test]
    fn a_crate_is_moved_by_its_whole_name_and_the_same_name_by_no_move() {
        let between = |gone: &str, candidate: &str| path(gone).moves_between(&path(candidate));
        // The same name moves nowhere.
        assert_eq!(between("old::Foo", "old::Foo"), []);
        // A crate alone, or the end of the other name: no segment is left to share, and
        // the whole of both is the one move.
        assert_eq!(between("old", "new"), [mv("old", "new")]);
        assert_eq!(between("old", "x::old"), [mv("old", "x::old")]);
        assert_eq!(between("x::old", "old"), [mv("x::old", "old")]);
    }

    #[test]
    fn a_member_has_its_type_as_owner_and_a_crate_or_an_item_of_one_has_none() {
        assert_eq!(
            path("demo::Shelf::put").owner(),
            Some((path("demo::Shelf"), "put"))
        );
        assert_eq!(
            path("demo::m::Shelf::put").owner(),
            Some((path("demo::m::Shelf"), "put"))
        );
        assert_eq!(path("demo::Shelf").owner(), None);
        assert_eq!(path("demo").owner(), None);
        assert_eq!(
            path("demo::Shelf").with("put"),
            Some(path("demo::Shelf::put"))
        );
        assert_eq!(path("demo::Shelf").with("put it"), None);
        assert_eq!(path("demo::Shelf").with("self"), None);
    }

    #[test]
    fn a_path_is_made_of_segments_by_the_rule_of_the_grammar() {
        let segments = |parts: &[&str]| parts.iter().map(|p| (*p).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            RustPath::of(&segments(&["demo", "m", "Foo"])),
            Some(path("demo::m::Foo"))
        );
        assert_eq!(RustPath::of(&[]), None);
        for wrong in [
            vec!["demo", ""],
            vec!["demo", "static.files"],
            vec!["demo", "r#type"],
            vec!["crate", "Foo"],
            vec!["my-crate"],
        ] {
            assert_eq!(RustPath::of(&segments(&wrong)), None, "{wrong:?}");
        }
    }

    /// Every path of one to five segments over three identifiers.
    fn every_path() -> Vec<RustPath> {
        paths_up_to(5)
    }

    /// Every path of one to `longest` segments over three identifiers.
    fn paths_up_to(longest: usize) -> Vec<RustPath> {
        let mut paths: Vec<Vec<&str>> = vec![vec![]];
        let mut all = Vec::new();
        for _ in 0..longest {
            paths = paths
                .into_iter()
                .flat_map(|prefix| {
                    ["a", "b", "c"].map(|segment| {
                        let mut next = prefix.clone();
                        next.push(segment);
                        next
                    })
                })
                .collect();
            all.extend(paths.iter().map(|segments| path(&segments.join("::"))));
        }
        all
    }

    /// The property of the adapter's suggestion: for every pair of names, every move it
    /// returns is one under which the renamed core maps the first to the second.
    #[test]
    fn the_renamed_core_maps_a_name_to_its_candidate_under_every_move_between_them() {
        let paths = every_path();
        let mut moves = 0;
        for gone in &paths {
            for candidate in &paths {
                for of in gone.moves_between(candidate) {
                    moves += 1;
                    let gone = name(&gone.as_name());
                    let (_, answers) = renamed(&BTreeSet::from([gone.clone()]), &of).unwrap();
                    assert_eq!(
                        answers.get(&gone),
                        Some(&Renamed::Under(name(&candidate.as_name()))),
                        "{gone} to {} under {of:?}",
                        candidate.as_name()
                    );
                }
            }
        }
        assert!(moves > 1000, "{moves}");
    }

    /// The completeness of the suggestion, found without its formula: for every pair of
    /// names, the moves it returns are exactly those of a leading part of the first (or
    /// the whole of it) to a leading part of the second (or the whole of it), both of a
    /// segment or more, under which the renamed core maps the first to the second; each
    /// pair of parts is tried by brute force, in the order of the widest first (the
    /// fewest segments in the part of the first name). The names are of one to four
    /// segments: every pair of parts of them is tried, which five would make slow.
    #[test]
    fn the_moves_between_two_names_are_every_move_under_which_the_renamed_core_maps_one_to_the_other()
     {
        let leading = |path: &RustPath, count: usize| -> NamePrefix {
            NamePrefix::new(&path.segments()[..count].join("::")).unwrap()
        };
        let paths = paths_up_to(4);
        let mut moves = 0;
        for gone in &paths {
            let gone_name = name(&gone.as_name());
            for candidate in &paths {
                let mut expected = Vec::new();
                for from in 1..=gone.segments().len() {
                    for to in 1..=candidate.segments().len() {
                        let Ok(of) = PrefixMove::new(leading(gone, from), leading(candidate, to))
                        else {
                            continue;
                        };
                        let (_, answers) =
                            renamed(&BTreeSet::from([gone_name.clone()]), &of).unwrap();
                        if answers.get(&gone_name)
                            == Some(&Renamed::Under(name(&candidate.as_name())))
                        {
                            expected.push(of);
                        }
                    }
                }
                moves += expected.len();
                assert_eq!(
                    gone.moves_between(candidate),
                    expected,
                    "{} to {}",
                    gone.as_name(),
                    candidate.as_name()
                );
            }
        }
        assert!(moves > 1000, "{moves}");
    }
}
