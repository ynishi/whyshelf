//! Who made a judgement: a name, perhaps a mail address, and whether a person or
//! an agent stood behind it.
//!
//! A [`Judge`] is what a [`Judgement`](super::judgement::Judgement) records as the
//! one who decided. It is a value: the domain does not read the environment, a
//! Git configuration or a client's introduction. The application reads those and
//! hands the result over.
//!
//! # Naming
//!
//! [`actor::Actor`](super::actor::Actor) is the bare name that the approvals of the
//! event log carry (`by`). It stays as it is for that log. `Judge` is the one the
//! ledger uses: a name that is normalised so that two spellings of one name compare
//! equal, an optional mail, and a [`Principal`]. The two have different names so
//! that neither can stand where the other is asked for.
//!
//! # Principal
//!
//! | principal | meaning |
//! |---|---|
//! | [`Principal::Human`] | declared by the person's own settings; a process's environment cannot declare it |
//! | [`Principal::Agent`] | a tool declared itself (the environment, or the client of the tool server) |
//! | [`Principal::Undeclared`] | nothing declared; the default, so that an agent typing at a shell is not recorded as a person |
//!
//! None of this authenticates anyone, as a Git author does not. Who may do what
//! (an agent adopting, say) is decided by the layer that knows the principal; this
//! module only carries it.
//!
//! # Same person
//!
//! [`Judge::same_person`] is how "my own record" is told. It compares the
//! normalised name and, when both sides have one, the mail. It does not compare the
//! principal: the person who approves through an agent and then revokes at a
//! terminal is still one person.

use std::fmt;

use super::canon::{Canon, ToCanon};
use super::error::JudgeError;

const NAME_MAX: usize = 128;
const EMAIL_MAX: usize = 254;
const TOOL_MAX: usize = 64;

/// The name of a judge, normalised.
///
/// Leading and trailing white space is removed and each run of white space inside
/// becomes one space, so that `"Ada  Lovelace"` and `"Ada Lovelace"` are one name.
/// A name is one line: a line break or another control character is refused (a tab
/// is white space). Case is kept: folding it would be a guess about someone's name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PersonName(String);

impl PersonName {
    /// A name, one short line.
    pub fn new(text: &str) -> Result<Self, JudgeError> {
        if text.chars().any(|c| c.is_control() && c != '\t') {
            return Err(JudgeError::NameBadShape);
        }
        let name = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if name.is_empty() {
            return Err(JudgeError::NameEmpty);
        }
        if name.chars().count() > NAME_MAX {
            return Err(JudgeError::NameBadShape);
        }
        Ok(Self(name))
    }

    /// The normalised name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PersonName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for PersonName {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// A mail address, lower-cased.
///
/// Only the outline is checked (one `@`, something on each side, no white space):
/// whether the address exists is not a question for the model. It is lower-cased so
/// that the spellings of one address compare equal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Email(String);

impl Email {
    /// An address of the shape `local@domain`.
    pub fn new(text: &str) -> Result<Self, JudgeError> {
        let text = text.trim();
        let shaped = text.len() <= EMAIL_MAX
            && !text.chars().any(|c| c.is_whitespace() || c.is_control())
            && matches!(text.split_once('@'),
                Some((local, domain)) if !local.is_empty()
                    && !domain.is_empty()
                    && !domain.contains('@'));
        if shaped {
            Ok(Self(text.to_lowercase()))
        } else {
            Err(JudgeError::EmailBadShape)
        }
    }

    /// The address.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Email {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for Email {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// The name an agent's tool gave itself.
///
/// One short line as given, trimmed. The model does not know any tool by name; the
/// value is whatever the tool declared.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolName(String);

impl ToolName {
    /// A tool's name, one short line.
    pub fn new(text: &str) -> Result<Self, JudgeError> {
        let name = text.trim();
        if name.is_empty() {
            return Err(JudgeError::ToolEmpty);
        }
        if name.chars().count() > TOOL_MAX || name.chars().any(char::is_control) {
            return Err(JudgeError::ToolBadShape);
        }
        Ok(Self(name.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ToCanon for ToolName {
    fn to_canon(&self) -> Canon {
        Canon::str(self.0.as_str())
    }
}

/// What stood behind a judgement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Principal {
    /// A person, declared by the person's own settings.
    Human,
    /// An agent, by the tool that declared itself.
    Agent {
        /// The tool.
        tool: ToolName,
    },
    /// Nothing was declared.
    Undeclared,
}

impl ToCanon for Principal {
    fn to_canon(&self) -> Canon {
        match self {
            Self::Human => Canon::map([("principal", Canon::str("human"))]),
            Self::Agent { tool } => Canon::map([
                ("principal", Canon::str("agent")),
                ("tool", tool.to_canon()),
            ]),
            Self::Undeclared => Canon::map([("principal", Canon::str("undeclared"))]),
        }
    }
}

/// The one who made a judgement.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Judge {
    name: PersonName,
    email: Option<Email>,
    principal: Principal,
}

impl Judge {
    /// A judge from the values the application read.
    ///
    /// The mail is `None` unless the person chose to record it: a shared ledger is a
    /// shared log, and a person's data is not put in it by default.
    pub fn new(name: PersonName, email: Option<Email>, principal: Principal) -> Self {
        Self {
            name,
            email,
            principal,
        }
    }

    /// The name.
    pub fn name(&self) -> &PersonName {
        &self.name
    }

    /// The mail, if it was recorded.
    pub fn email(&self) -> Option<&Email> {
        self.email.as_ref()
    }

    /// What stood behind the judgement.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// Whether the two are one person: the same name, and the same mail when both
    /// have one. The principal does not count.
    pub fn same_person(&self, other: &Judge) -> bool {
        self.name == other.name
            && match (&self.email, &other.email) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

impl fmt::Display for Judge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name.as_str())
    }
}

impl ToCanon for Judge {
    fn to_canon(&self) -> Canon {
        Canon::map([
            ("name", self.name.to_canon()),
            ("email", self.email.to_canon()),
            ("principal", self.principal.to_canon()),
        ])
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn judge(name: &str) -> Judge {
        Judge::new(PersonName::new(name).unwrap(), None, Principal::Human)
    }

    pub(crate) fn agent(name: &str, tool: &str) -> Judge {
        Judge::new(
            PersonName::new(name).unwrap(),
            None,
            Principal::Agent {
                tool: ToolName::new(tool).unwrap(),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn a_name_is_one_normalised_line() {
        assert_eq!(
            PersonName::new("  Ada \t Lovelace ").unwrap().as_str(),
            "Ada Lovelace"
        );
        assert_eq!(PersonName::new("  "), Err(JudgeError::NameEmpty));
        assert_eq!(PersonName::new("a\nb"), Err(JudgeError::NameBadShape));
        assert_eq!(
            PersonName::new(&"a".repeat(129)),
            Err(JudgeError::NameBadShape)
        );
        assert_eq!(
            PersonName::new("Ada").unwrap(),
            PersonName::new(" Ada ").unwrap()
        );
    }

    #[test]
    fn a_mail_has_an_outline_and_is_lower_cased() {
        assert_eq!(
            Email::new(" Ada@Example.ORG ").unwrap().as_str(),
            "ada@example.org"
        );
        for bad in ["", "ada", "@x", "ada@", "a@b@c", "a b@c", "a@b\nc"] {
            assert_eq!(Email::new(bad), Err(JudgeError::EmailBadShape), "{bad:?}");
        }
    }

    #[test]
    fn a_tool_name_is_one_short_line() {
        assert_eq!(ToolName::new(" my tool ").unwrap().as_str(), "my tool");
        assert_eq!(ToolName::new(""), Err(JudgeError::ToolEmpty));
        assert_eq!(ToolName::new("a\tb"), Err(JudgeError::ToolBadShape));
        assert_eq!(
            ToolName::new(&"t".repeat(65)),
            Err(JudgeError::ToolBadShape)
        );
    }

    #[test]
    fn the_principal_does_not_make_another_person() {
        assert!(judge("ada").same_person(&agent("ada", "some-agent")));
        assert!(!judge("ada").same_person(&judge("grace")));
    }

    #[test]
    fn mails_tell_two_people_of_one_name_apart_only_when_both_have_one() {
        let with = |mail: Option<&str>| {
            Judge::new(
                PersonName::new("ada").unwrap(),
                mail.map(|m| Email::new(m).unwrap()),
                Principal::Human,
            )
        };
        assert!(with(Some("a@x.org")).same_person(&with(Some("A@X.org"))));
        assert!(!with(Some("a@x.org")).same_person(&with(Some("b@x.org"))));
        assert!(with(Some("a@x.org")).same_person(&with(None)));
    }

    #[test]
    fn the_canonical_form_separates_the_principals() {
        let forms: Vec<Vec<u8>> = [
            judge("ada"),
            agent("ada", "t"),
            Judge::new(PersonName::new("ada").unwrap(), None, Principal::Undeclared),
        ]
        .iter()
        .map(|j| j.to_canon().to_bytes())
        .collect();
        assert_ne!(forms[0], forms[1]);
        assert_ne!(forms[0], forms[2]);
        assert_ne!(forms[1], forms[2]);
    }
}
