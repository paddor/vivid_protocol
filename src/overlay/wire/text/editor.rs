//! Revision-scoped document editing for the optional `overlay-editor-v1` profile.

// Rust guideline compliant 2026-10-07
use super::super::{
    MessageError, PayloadMap, Value, WindowAddress, array, bad, boolean, nonzero, small, strict, u,
};
use std::{fmt, ops::Range};

/// Maximum UTF-8 bytes in a document excerpt, commit or preedit.
pub const MAX_TEXT_BYTES: usize = super::super::super::MAX_EVENT_TEXT_BYTES;

/// A bounded document excerpt excluding the current native preedit.
#[derive(Clone, PartialEq, Eq)]
pub struct Context {
    /// Nonzero editor incarnation; advance on field replacement or external edits.
    pub editor: u64,
    /// Nonzero revision of this excerpt and selection.
    pub revision: u64,
    /// UTF-8 text around the complete current selection.
    pub text: String,
    /// Cursor UTF-8 byte offset in the excerpt.
    pub cursor: u32,
    /// Selection anchor UTF-8 byte offset; may precede or follow the cursor.
    pub anchor: u32,
    /// Sensitive editors publish no surrounding text or document selection.
    pub sensitive: bool,
}
impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorContext")
            .field("editor", &self.editor)
            .field("revision", &self.revision)
            .field("bytes", &self.text.len())
            .finish_non_exhaustive()
    }
}
impl Context {
    /// Validates identity, byte limits, selection boundaries and sensitive masking.
    ///
    /// # Errors
    /// Rejects zero identities, oversized text, invalid byte offsets or unmasked sensitive text.
    pub fn validate(&self) -> Result<(), MessageError> {
        nonzero(self.editor, 4)?;
        nonzero(self.revision, 5)?;
        if self.text.len() > MAX_TEXT_BYTES
            || !self.text.is_char_boundary(self.cursor as usize)
            || !self.text.is_char_boundary(self.anchor as usize)
            || (self.sensitive && (!self.text.is_empty() || self.cursor != 0 || self.anchor != 0))
        {
            return Err(bad(6, "invalid editor context"));
        }
        Ok(())
    }
}

/// Publishes or clears the focused displayed window's native document context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetContext {
    /// Current window address.
    pub address: WindowAddress,
    /// Nonzero actually displayed scene revision.
    pub scene_revision: u64,
    /// Current excerpt, or None to clear document editing while retaining composition.
    pub context: Option<Context>,
}
impl SetContext {
    /// Encodes a strict `SET_OVERLAY_TEXT_CONTEXT` payload.
    ///
    /// # Errors
    /// Rejects invalid window identity, scene revision or context.
    pub fn payload(&self) -> Result<PayloadMap, MessageError> {
        self.address.validate(self.address.surface_id)?;
        nonzero(self.scene_revision, 3)?;
        let value = if let Some(context) = &self.context {
            context.validate()?;
            Value::Array(vec![
                u(context.editor),
                u(context.revision),
                Value::Text(context.text.clone()),
                u(context.cursor.into()),
                u(context.anchor.into()),
                Value::Bool(context.sensitive),
            ])
        } else {
            Value::Null
        };
        let mut payload = self.address.payload();
        payload.extend([(3, u(self.scene_revision)), (4, value)]);
        Ok(payload)
    }
    /// Decodes a context after validating borrowed text before cloning it.
    ///
    /// # Errors
    /// Rejects unknown fields, invalid addresses, revisions, text or selection.
    pub fn decode(object: u64, value: &Value) -> Result<Self, MessageError> {
        let map = strict(value, &[0, 1, 2, 3, 4])?;
        let context = match map.required(4)? {
            Value::Null => None,
            value => {
                let fields = array::<6>(value, 4)?;
                let text = fields[2]
                    .as_text()
                    .filter(|s| s.len() <= MAX_TEXT_BYTES)
                    .ok_or_else(|| bad(4, "invalid editor text"))?;
                let context = Context {
                    editor: nonzero(
                        fields[0]
                            .as_u64()
                            .ok_or_else(|| bad(4, "invalid editor identity"))?,
                        4,
                    )?,
                    revision: nonzero(
                        fields[1]
                            .as_u64()
                            .ok_or_else(|| bad(4, "invalid editor revision"))?,
                        4,
                    )?,
                    text: text.to_owned(),
                    cursor: small(&fields[3], 4)?,
                    anchor: small(&fields[4], 4)?,
                    sensitive: boolean(&fields[5], 4)?,
                };
                context.validate()?;
                Some(context)
            }
        };
        Ok(Self {
            address: WindowAddress::decode(object, &map)?,
            scene_revision: nonzero(map.required_u64(3)?, 3)?,
            context,
        })
    }
}

/// One atomic document selection, replacement, commit and preedit operation.
#[derive(Clone, PartialEq, Eq)]
pub struct Edit {
    /// Original window address.
    pub address: WindowAddress,
    /// Actually displayed scene used for admission.
    pub scene_revision: u64,
    /// Original editor incarnation.
    pub editor: u64,
    /// Excerpt revision required for absolute selection and replacement.
    pub revision: u64,
    /// Absolute excerpt range; None uses the editor's current selection/preedit.
    pub replacement: Option<Range<u32>>,
    /// Standalone document cursor and anchor update, including reversed selections.
    pub selection: Option<(u32, u32)>,
    /// Optional committed text; Some(empty) deletes the replacement/selection.
    pub commit: Option<String>,
    /// Optional new preedit and its ordered UTF-8 selection; empty text clears it.
    pub preedit: Option<(String, Option<Range<u32>>)>,
}
impl fmt::Debug for Edit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorEdit")
            .field("address", &self.address)
            .field("editor", &self.editor)
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
fn range_value(range: &Range<u32>) -> Value {
    Value::Array(vec![u(range.start.into()), u(range.end.into())])
}
fn decode_pair(value: &Value, key: u64) -> Result<(u32, u32), MessageError> {
    let values = array::<2>(value, key)?;
    Ok((small(&values[0], key)?, small(&values[1], key)?))
}
impl Edit {
    /// Checks syntax and byte limits independently of current editor authority.
    ///
    /// # Errors
    /// Rejects malformed identities, oversized text, invalid ranges or mixed selection operations.
    pub fn validate(&self) -> Result<(), MessageError> {
        self.address.validate(self.address.surface_id)?;
        nonzero(self.scene_revision, 3)?;
        nonzero(self.editor, 4)?;
        nonzero(self.revision, 5)?;
        if self
            .commit
            .as_ref()
            .is_some_and(|s| s.len() > MAX_TEXT_BYTES)
            || self
                .replacement
                .as_ref()
                .is_some_and(|r| r.start > r.end || r.end as usize > MAX_TEXT_BYTES)
            || (self.replacement.is_some() && self.commit.is_none() && self.preedit.is_none())
            || self
                .selection
                .is_some_and(|(a, b)| a as usize > MAX_TEXT_BYTES || b as usize > MAX_TEXT_BYTES)
            || (self.selection.is_some()
                && (self.commit.is_some() || self.preedit.is_some() || self.replacement.is_some()))
            || self.preedit.as_ref().is_some_and(|(text, selection)| {
                text.len() > MAX_TEXT_BYTES
                    || selection.as_ref().is_some_and(|r| {
                        r.start > r.end
                            || !text.is_char_boundary(r.start as usize)
                            || !text.is_char_boundary(r.end as usize)
                    })
            })
        {
            return Err(bad(6, "invalid editor edit"));
        }
        Ok(())
    }
    /// Checks the edit against its original editor and current excerpt.
    ///
    /// Selection-relative edits survive their own preceding replies. Absolute
    /// ranges require the exact revision and must cover the complete selection.
    /// Producers must advance `editor` for external changes before accepting more edits.
    ///
    /// # Errors
    /// Rejects stale editors/revisions, sensitive absolute edits and invalid UTF-8 ranges.
    pub fn validate_context(&self, context: &Context) -> Result<(), MessageError> {
        self.validate()?;
        context.validate()?;
        if self.editor != context.editor
            || self.revision > context.revision
            || ((self.replacement.is_some() || self.selection.is_some())
                && (self.revision != context.revision || context.sensitive))
        {
            return Err(bad(4, "stale or sensitive editor context"));
        }
        if self.replacement.as_ref().is_some_and(|r| {
            !context.text.is_char_boundary(r.start as usize)
                || !context.text.is_char_boundary(r.end as usize)
                || r.start > context.cursor.min(context.anchor)
                || r.end < context.cursor.max(context.anchor)
        }) || self.selection.is_some_and(|(a, b)| {
            !context.text.is_char_boundary(a as usize) || !context.text.is_char_boundary(b as usize)
        }) {
            return Err(bad(6, "edit range does not match editor context"));
        }
        Ok(())
    }
    /// Encodes a strict `OVERLAY_TEXT_EDIT` payload.
    ///
    /// # Errors
    /// Returns invalid identity, text, range or selection errors.
    pub fn payload(&self) -> Result<PayloadMap, MessageError> {
        self.validate()?;
        let mut fields = self.address.payload();
        fields.extend([
            (3, u(self.scene_revision)),
            (4, u(self.editor)),
            (5, u(self.revision)),
            (
                6,
                self.replacement.as_ref().map_or(Value::Null, range_value),
            ),
            (
                7,
                self.selection.map_or(Value::Null, |(a, b)| {
                    Value::Array(vec![u(a.into()), u(b.into())])
                }),
            ),
            (
                8,
                self.commit
                    .as_ref()
                    .map_or(Value::Null, |s| Value::Text(s.clone())),
            ),
            (
                9,
                self.preedit.as_ref().map_or(Value::Null, |(s, r)| {
                    Value::Array(vec![
                        Value::Text(s.clone()),
                        r.as_ref().map_or(Value::Null, range_value),
                    ])
                }),
            ),
        ]);
        Ok(fields)
    }
    /// Decodes a strict edit, bounding borrowed text before cloning it.
    ///
    /// # Errors
    /// Rejects unknown fields, invalid values and oversized text.
    pub fn decode(object: u64, value: &Value) -> Result<Self, MessageError> {
        let map = strict(value, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9])?;
        let pair = |key| -> Result<Option<(u32, u32)>, MessageError> {
            match map.required(key)? {
                Value::Null => Ok(None),
                v => decode_pair(v, key).map(Some),
            }
        };
        let text = |v: &Value| -> Result<String, MessageError> {
            v.as_text()
                .filter(|s| s.len() <= MAX_TEXT_BYTES)
                .map(str::to_owned)
                .ok_or_else(|| bad(8, "invalid editor text"))
        };
        let result = Self {
            address: WindowAddress::decode(object, &map)?,
            scene_revision: map.required_u64(3)?,
            editor: map.required_u64(4)?,
            revision: map.required_u64(5)?,
            replacement: pair(6)?.map(|(a, b)| a..b),
            selection: pair(7)?,
            commit: match map.required(8)? {
                Value::Null => None,
                v => Some(text(v)?),
            },
            preedit: match map.required(9)? {
                Value::Null => None,
                v => {
                    let fields = array::<2>(v, 9)?;
                    Some((
                        text(&fields[0])?,
                        match &fields[1] {
                            Value::Null => None,
                            v => {
                                let (a, b) = decode_pair(v, 9)?;
                                Some(a..b)
                            }
                        },
                    ))
                }
            },
        };
        result.validate()?;
        Ok(result)
    }
}
