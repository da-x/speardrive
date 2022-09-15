// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use derive_builder::Builder;

use gitlab::api::{common::NameOrId, endpoint_prelude::*};

/// Query for jobs within a pipeline.
#[derive(Debug, Builder)]
pub struct JobArtifacts<'a> {
    /// The project to query for the pipeline.
    #[builder(setter(into))]
    project: NameOrId<'a>,
    /// The ID of the job.
    job: u64,
}

impl<'a> JobArtifacts<'a> {
    /// Create a builder for the endpoint.
    pub fn builder() -> JobArtifactsBuilder<'a> {
        JobArtifactsBuilder::default()
    }
}

impl<'a> Endpoint for JobArtifacts<'a> {
    fn method(&self) -> Method {
        Method::GET
    }

    fn endpoint(&self) -> Cow<'static, str> {
        format!("projects/{}/jobs/{}/artifacts", self.project, self.job).into()
    }

    fn parameters(&self) -> QueryParams {
        QueryParams::default()
    }
}
