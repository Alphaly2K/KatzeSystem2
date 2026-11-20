use super::ModelConfig;
use crate::{
    Result,
    host::{
        resources::{ResourceMode, ResourceServices},
        storage::SharedSource,
    },
};

/// The profile, body and initial motion from an expanded FES model selection.
/// Selectors retain the script's byte encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelSelection<'a> {
    pub profile: &'a [u8],
    pub body: &'a [u8],
    pub motion: &'a [u8],
    /// Remaining comma-separated fields for the host player.
    pub options: &'a [u8],
}

/// The resources needed to open the selected character body.
#[derive(Clone)]
pub struct ModelSource {
    /// Packed PSZ data. The host decides when and how much to decode.
    pub psz: SharedSource,
    /// Name of the initial motion in the model, if the profile defines one.
    pub initial_motion: Option<Vec<u8>>,
    /// Optional difference tables in the `m2config` category.
    pub expression_config: Option<Vec<u8>>,
    pub body_config: Option<Vec<u8>>,
}

impl<'a> ModelSelection<'a> {
    /// Parse a comma-separated selection. Empty or omitted body and motion selectors
    /// default to `1` and `0`. An empty profile has no selection.
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        let bytes = bytes.split(|&byte| byte == 0).next().unwrap_or_default();
        let mut fields = bytes.splitn(4, |&byte| byte == b',');
        let profile = fields.next()?.trim_ascii();
        if profile.is_empty() {
            return None;
        }
        let body = fields
            .next()
            .map(<[u8]>::trim_ascii)
            .filter(|field| !field.is_empty());
        let motion = fields
            .next()
            .map(<[u8]>::trim_ascii)
            .filter(|field| !field.is_empty());
        Some(Self {
            profile,
            body: body.unwrap_or(b"1"),
            motion: motion.unwrap_or(b"0"),
            options: fields.next().unwrap_or_default(),
        })
    }

    /// Resolve and open the PSZ selected by the profile with loose resources
    /// taking precedence. A missing profile, body or PSZ returns `None`.
    pub fn open(self, resources: &mut dyn ResourceServices) -> Result<Option<ModelSource>> {
        let mut profile = self.profile.to_vec();
        if !profile
            .last_chunk::<4>()
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(b".pfl"))
        {
            profile.extend_from_slice(b".pfl");
        }
        let name = resources.resolve(b"m2config", &profile)?;
        let Some(bytes) = resources.read(&name, ResourceMode::LooseFirst)? else {
            return Ok(None);
        };
        let config = ModelConfig::parse(&bytes)?;
        let Some(body) = config
            .body(self.body)
            .filter(|body| !body.resource.is_empty())
        else {
            return Ok(None);
        };
        let name = resources.resolve(b"m2psb", body.resource)?;
        let Some(psz) = resources.open(&name, ResourceMode::LooseFirst)? else {
            return Ok(None);
        };
        let mut source = ModelSource {
            psz,
            initial_motion: None,
            expression_config: None,
            body_config: None,
        };
        if let Some(motion) = config.motion(self.body, self.motion) {
            let optional_name = |bytes: &[u8]| (!bytes.is_empty()).then(|| bytes.to_vec());
            let mut names = motion.names();
            source.initial_motion = optional_name(motion.label);
            source.expression_config = names.next().and_then(optional_name);
            source.body_config = names.next().and_then(optional_name);
        }
        Ok(Some(source))
    }
}
