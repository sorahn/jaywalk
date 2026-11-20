use super::*;

/// Provider-published paths and immutable endpoints, before any file metadata
/// requests. Kept separate from fully hydrated Data so an unresolved file cannot
/// accidentally be presented as an ordinary, non-executable text file.
pub(super) struct FileSpec {
    pub change: ChangedFile,
    pub before: Option<Endpoint>,
    pub after: Option<Endpoint>,
}
pub(super) struct Verification {
    url: Url,
    fields: Vec<(&'static str, Option<Value>)>,
}
impl Verification {
    pub fn new(url: Url, detail: &Value, pointers: &[&'static str]) -> Self {
        Self {
            url,
            fields: pointers
                .iter()
                .map(|&p| (p, detail.pointer(p).cloned()))
                .collect(),
        }
    }
}
pub(super) struct Manifest {
    pub metadata: Metadata,
    pub files: Vec<FileSpec>,
    pub commits: Vec<Revision>,
    pub verification: Verification,
}
impl Manifest {
    pub fn verify(
        &self,
        control: &Control,
        request: &mut impl FnMut(Request) -> Result<Reply, Error>,
    ) -> Result<(), Error> {
        control.check()?;
        let current = json(request(Request::Json(self.verification.url.clone()))?)?;
        if self
            .verification
            .fields
            .iter()
            .any(|(p, v)| current.pointer(p) != v.as_ref())
        {
            return Err(Error::Superseded);
        }
        control.check()
    }
    pub fn hosted(
        &self,
        control: &Control,
        request: &mut impl FnMut(Request) -> Result<Reply, Error>,
    ) -> Result<Data, Error> {
        control.check()?;
        let files = match self.metadata.locator.provider {
            crate::accounts::Provider::Github => {
                github::hydrate_specs(&self.files, control, request)?
            }
            crate::accounts::Provider::Bitbucket => bitbucket::hydrate_specs(&self.files, request)?,
        };
        Ok(Data {
            metadata: self.metadata.clone(),
            commits: self.commits.clone(),
            files,
        })
    }
    pub fn local(
        &self,
        workspace: &crate::local_review::Workspace,
        control: &Control,
    ) -> Result<Data, String> {
        workspace.tree(&self.metadata.base.commit, control)?;
        workspace.tree(&self.metadata.source.commit, control)?;
        let blob = |endpoint: &Endpoint, path: &str| -> Result<Blob, String> {
            control.check().map_err(|e| e.to_string())?;
            let entry = workspace.entry(&endpoint.commit, path, control)?;
            let url = match self.metadata.locator.provider {
                crate::accounts::Provider::Github => url(
                    crate::accounts::Provider::Github,
                    &endpoint.repository.name,
                    &["git", "blobs", &entry.object],
                ),
                crate::accounts::Provider::Bitbucket => {
                    bitbucket::raw_url(&endpoint.repository.name, &endpoint.commit, path)
                }
            }
            .map_err(|e| e.to_string())?;
            Ok(Blob {
                url,
                kind: entry.kind,
                executable: entry.executable,
            })
        };
        let files = self
            .files
            .iter()
            .map(|spec| {
                Ok(File {
                    change: spec.change.clone(),
                    before: spec
                        .before
                        .as_ref()
                        .map(|ep| blob(ep, spec.change.before_path()))
                        .transpose()?,
                    after: spec
                        .after
                        .as_ref()
                        .map(|ep| blob(ep, spec.change.after_path()))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Data {
            metadata: self.metadata.clone(),
            commits: self.commits.clone(),
            files,
        })
    }
}
