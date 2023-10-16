use std::path::{PathBuf, Path};
use std::sync::Arc;
use std::{
    collections::{HashMap, VecDeque},
    convert::Infallible,
    net::ToSocketAddrs,
    str::FromStr,
};

use cmdline::CommandArgs;
use error::Error;
use fs2::FileExt;
use gitlab::{api::AsyncQuery, AsyncGitlab, GitlabBuilder};
use hyper::StatusCode;
use hyper::{
    http::uri::PathAndQuery,
    service::{make_service_fn, service_fn},
    Body, Request, Response, Uri,
};
use regex::Regex;
use structopt::StructOpt;

mod artifacts;
mod cmdline;
mod config;
mod error;
mod logging;
mod util;

use crate::config::{Config, GitlabJobSource, LocalPathSource, RemoteSource};

struct Main {
    config: Config,
}

#[derive(Debug, Clone)]
struct Plan {
    artifacts: Vec<Artifact>,
    sub_uri: String,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Kind {
    RPM,
}

#[derive(Debug, Clone)]
enum Artifact {
    GitlabJob(JobArtifact),
    Local(LocalArtifact),
    Remote(StaticRemoteArtifact),
}

#[derive(Debug, Clone)]
struct JobArtifact {
    source_name: String,
    project: String,
    job_id: u64,
}

#[derive(Debug, Clone)]
struct LocalArtifact {
    source_name: String,
    key: PathBuf,
}

#[derive(Debug, Clone)]
struct StaticRemoteArtifact {
    source_name: String,
    subpath: String,
}

impl Plan {
    fn to_composite_path(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        let without_suburi = Self {
            sub_uri: "".to_owned(),
            ..(*self).clone()
        };
        let rep = format!("{:?}", without_suburi);
        hasher.update(rep.as_bytes());
        let result = hasher.finalize();

        return format!("{}", hex::encode(result));
    }

    fn from_uri(uri: &str, config: &Arc<Config>) -> Result<Plan, Error> {
        let mut artifacts = vec![];

        let comps = uri.split("/").collect::<Vec<&str>>();
        if comps.len() <= 2 {
            return Err(Error::PlanParse("not enough components".to_owned()));
        }

        let mut sub_uri = String::new();
        let kind = Kind::RPM;

        for item in comps[1..].join("/").split("/-/") {
            lazy_static::lazy_static! {
                static ref RE: Regex = Regex::new("[/a-z0-9_-]+").unwrap();
            }

            let mut parts: VecDeque<_> = item.split("/").collect();

            let prefix = if let Some(f) = parts.pop_front() {
                f
            } else {
                continue;
            };

            if prefix == "rpm" {
                sub_uri = format!("/{}", parts.into_iter().collect::<Vec<_>>().join("/"));
                continue;
            }

            // For sanity, remove parts that can be '..'.
            let mut parts: VecDeque<_> = parts.into_iter().filter(|x| *x != "..").collect();

            if let Some(_) = config.gitlabs.get(prefix) {
                if let Some(job_id) = parts.pop_back() {
                    let project = parts.into_iter().collect::<Vec<_>>().join("/");

                    if !RE.is_match(&project) {
                        return Err(Error::PlanParse(format!(
                            "{} invalid project name",
                            project
                        )));
                    }

                    artifacts.push(Artifact::GitlabJob(JobArtifact {
                        source_name: prefix.to_owned(),
                        project,
                        job_id: job_id.parse()?,
                    }))
                }
            } else if let Some(_) = config.local_source.get(prefix) {
                if let Some(key) = parts.pop_back() {
                    if key != ".." {
                        artifacts.push(Artifact::Local(LocalArtifact {
                            source_name: prefix.to_owned(),
                            key: key.into(),
                        }))
                    }
                }
            } else if let Some(_) = config.remote_source.get(prefix) {
                artifacts.push(Artifact::Remote(StaticRemoteArtifact {
                    subpath: parts.into_iter().collect::<Vec<_>>().join("/"),
                    source_name: prefix.to_owned(),
                }))
            } else {
                return Err(Error::UnknownSource(prefix.into()));
            }
        }

        Ok(Plan {
            artifacts,
            sub_uri,
            kind,
        })
    }
}

struct ClientCache {
    gitlab_clients: HashMap<String, AsyncGitlab>,
}

impl ClientCache {
    fn new() -> Self {
        Self {
            gitlab_clients: HashMap::new(),
        }
    }

    async fn get(
        &mut self,
        name: &String,
        gpipe: &GitlabJobSource,
    ) -> Result<&mut AsyncGitlab, Error> {
        if !self.gitlab_clients.contains_key(name) {
            let builder = GitlabBuilder::new(&gpipe.hostname, &gpipe.api_key);
            let gitlab = builder.build_async().await?;

            self.gitlab_clients.insert(name.clone(), gitlab);
        }

        Ok(self.gitlab_clients.get_mut(name).unwrap())
    }
}

async fn service_handle(config: Arc<Config>, req: Request<Body>) -> Result<Response<Body>, Error> {
    let uri = req.uri().to_string();
    log::info!("request: {}", uri);

    let plan = Plan::from_uri(&uri, &config)?;
    log::info!("request: plan - {:?}", plan);

    let mut gitlab = ClientCache::new();

    for artifact in plan.artifacts.iter() {
        match artifact {
            Artifact::GitlabJob(job) => {
                if let Some(gpipe) = config.gitlabs.get(&job.source_name) {
                    let project_path = config.local_cache.join(&job.source_name).join(&job.project);
                    let lock = project_path.join(format!("lock"));
                    let path_tmp = project_path.join(format!("{}.tmp", job.job_id));
                    let path = project_path.join(format!("{}", job.job_id));

                    if path.exists() {
                        log::info!("request: {}: artifacts {} exist", uri, path.display());
                        continue;
                    }

                    cache_gitlab_job_artifacts(
                        project_path,
                        lock,
                        path_tmp,
                        &job,
                        gpipe,
                        &uri,
                        &mut gitlab,
                        path,
                    )
                    .await?;
                }
            }
            Artifact::Remote(sra) => {
                if let Some(sr) = config.remote_source.get(&sra.source_name) {
                    let orig_path = config.local_cache.join(&sra.source_name);
                    let lock = orig_path.join(format!("lock"));
                    let path_tmp = orig_path.join(format!("{}.tmp", sra.subpath));
                    let path = orig_path.join(format!("{}", sra.subpath));

                    if path.exists() {
                        log::info!("request: {}: static remote copy {} exist", uri, path.display());
                        continue;
                    }

                    cache_static_remote_artifact(
                        orig_path,
                        lock,
                        path_tmp,
                        &sra,
                        sr,
                        &uri,
                        path,
                    )
                    .await?;
                }
            },
            Artifact::Local(_) => {}
        }
    }

    // Create composite directory
    let lock = config.composites_cache.join(format!("lock"));
    let node_name = plan.to_composite_path();
    let composite_path = config.composites_cache.join(&node_name);
    let path_tmp = config.composites_cache.join(format!("{}.tmp", node_name));

    if !composite_path.exists() {
        log::info!(
            "request: {}: creating composite path {}",
            uri,
            composite_path.display()
        );

        let _ = std::fs::create_dir_all(&config.composites_cache)?;

        let lockfile = std::fs::File::create(&lock)?;
        lockfile.lock_exclusive()?;

        let _ = std::fs::remove_dir_all(&path_tmp);
        std::fs::create_dir_all(&path_tmp)?;

        for (idx, artifact) in plan.artifacts.iter().enumerate() {
            let path_dest = path_tmp.join(format!("{idx}"));
            let path_dest = path_dest.display();

            let artifact_path = match artifact {
                Artifact::GitlabJob(job) => {
                    if let Some(_) = config.gitlabs.get(&job.source_name) {
                        let project_path =
                            config.local_cache.join(&job.source_name).join(&job.project);
                        Some(project_path.join(format!("{}", job.job_id)))
                    } else {
                        None
                    }
                }
                Artifact::Local(local) => {
                    if let Some(local_source) = config.local_source.get(&local.source_name) {
                        Some(local_source.root.join(&local.key))
                    } else {
                        None
                    }
                }
                Artifact::Remote(remote) => {
                    if let Some(_) = config.remote_source.get(&remote.source_name) {
                        let cache_path = config.local_cache.join(&remote.source_name);
                        Some(cache_path.join(&remote.subpath))
                    } else {
                        None
                    }
                }
            };

            if let Some(artifact_path) = artifact_path {
                let artifact_path = artifact_path.display();
                util::bash(format!(
                        "cp -al {artifact_path} {path_dest}/ || cp -a {artifact_path} {path_dest}/"
                ))?;
            }
        }

        std::fs::write(path_tmp.join("url.txt"), uri)?;

        match plan.kind {
            Kind::RPM => {
                let path_tmp = path_tmp.display();
                util::bash(format!("createrepo {path_tmp}"))?;
            }
        }

        std::fs::rename(path_tmp, &composite_path)?;
    }

    let static_ = hyper_staticfile::Static::new(&composite_path);

    let mut req = req;
    let mut parts = req.uri().clone().into_parts();
    if let Some(p) = &mut parts.path_and_query {
        *p = PathAndQuery::from_str(&plan.sub_uri).unwrap();
    }
    *req.uri_mut() = Uri::from_parts(parts)?;

    log::info!("request: serving from {}/{}", composite_path.display(), req.uri());

    Ok(static_.serve(req).await?)
}

async fn service_handle_wrapper(config: Arc<Config>, req: Request<Body>) -> Result<Response<Body>, Error> {
    let uri = req.uri().to_string();
    match service_handle(config, req).await {
        Ok(v) => Ok(v),
        Err(err) => {
            log::error!("request: {}, failed: {}", uri, err);
            let mut rsp = Response::new(Body::from(format!("{:?}", err)));
            *rsp.status_mut() = StatusCode::BAD_REQUEST;
            Ok(rsp)
        },
    }
}

async fn cache_gitlab_job_artifacts(
    project_path: PathBuf,
    lock: PathBuf,
    path_tmp: PathBuf,
    job: &JobArtifact,
    gpipe: &GitlabJobSource,
    uri: &String,
    gitlab: &mut ClientCache,
    path: PathBuf,
) -> Result<(), Error> {
    std::fs::create_dir_all(&project_path)?;

    let lockfile = std::fs::File::create(&lock)?;
    lockfile.lock_exclusive()?;

    log::info!(
        "request: {}: querying project '{}' job '{}'",
        uri,
        job.project,
        job.job_id
    );

    let _ = std::fs::remove_dir_all(&path_tmp);
    std::fs::create_dir_all(&path_tmp)?;
    let endpoint = artifacts::JobArtifacts::builder()
        .project(job.project.clone())
        .job(job.job_id)
        .build()
        .map_err(Error::BuilderError)?;

    log::info!("request: {}: downloading artifacts", uri);

    let content = gitlab::api::raw(endpoint)
        .query_async(gitlab.get(&job.source_name, gpipe).await?)
        .await
        .map_err(|x| Error::Boxed(Arc::new(x)))?;
    let artifacts_zip = path_tmp.join("artifacts_zip");
    std::fs::write(&artifacts_zip, content)?;

    log::info!("request: {}: extracting artifacts", uri);
    {
        let artifacts_zip = artifacts_zip.display();
        let path_tmp = path_tmp.display();
        util::bash(format!("unzip {artifacts_zip} -d {path_tmp}"))?;
    }

    log::info!("request: {}: placing artifacts", uri);

    std::fs::remove_file(artifacts_zip)?;
    std::fs::rename(path_tmp, path)?;

    Ok(())
}

async fn cache_static_remote_artifact(
    orig_path: PathBuf,
    lock: PathBuf,
    path_tmp: PathBuf,
    sra: &StaticRemoteArtifact,
    sr: &RemoteSource,
    uri: &String,
    path: PathBuf,
) -> Result<(), Error> {
    std::fs::create_dir_all(&orig_path)?;

    let lockfile = std::fs::File::create(&lock)?;
    lockfile.lock_exclusive()?;

    log::info!("request: {}: querying SRA {:?} of static remote {:?}", uri, sra, sr);

    let _ = std::fs::remove_dir_all(&path_tmp);
    std::fs::create_dir_all(&path_tmp)?;

    log::info!("request: {}: downloading SRA into {:?}", uri, path_tmp.display());

    let list_url = format!("{}/{}/list.txt", &sr.base_url, sra.subpath);
    let list_txt = reqwest::get(&list_url).await?.text().await?;

    for line in list_txt.lines() {
        // Sanitize the line
        let parts: Vec<_> = line.split("/").into_iter()
            .filter(|x| *x != "..")
            .skip_while(|x| *x == "").collect();
        let line = parts.join("/");
        let local_path = path_tmp.join(Path::new(&line));

        // Make sure the parent dir exists
        if let Some(parent) = local_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        // Download the file and write it
        let file_url = format!("{}/{}/{}", &sr.base_url, sra.subpath, line);
        log::info!("request: {}: downloading {}", uri, file_url);
        let content = reqwest::get(&file_url).await?.bytes().await?;
        tokio::fs::write(local_path, content).await?;
    }

    log::info!("request: {}: placing SRA", uri);
    std::fs::rename(path_tmp, path)?;

    Ok(())
}

impl Main {
    async fn new(opt: &CommandArgs) -> Result<Self, Error> {
        logging::activate(&opt.logging, logging::empty_filter)?;

        match &opt.cmd {
            cmdline::Command::ExampleConf => {
                println!(
                    "{}",
                    serde_yaml::to_string(&Config {
                        listen_addr: "127.0.0.1:4444".into(),
                        composites_cache: PathBuf::from("/storage/for/repo-composites"),
                        local_cache: PathBuf::from("/storage/for/cached-job-artifacts"),
                        local_source: vec![(
                            "local".into(),
                            LocalPathSource {
                                root: "/opt/repo/build-output".into(),
                            }
                        )]
                        .into_iter()
                        .collect(),
                        remote_source: vec![].into_iter().collect(),
                        gitlabs: vec![(
                            "myserver".into(),
                            GitlabJobSource {
                                api_key: "SomeAPIKEYObtainedFromGitlab".into(),
                                hostname: "git.myserver.com".into(),
                            }
                        )]
                        .into_iter()
                        .collect()
                    })?
                );
                return Err(Error::Help);
            }
            cmdline::Command::Serve => Ok(Self {
                config: Self::load_config(opt)?,
            }),
        }
    }

    fn load_config(opt: &CommandArgs) -> Result<Config, Error> {
        use ::config as cconfig;
        use cconfig::TranslationType;

        let config_path = if let Some(config) = &opt.config {
            Some(config.clone())
        } else {
            if let Ok(path) = std::env::var("SPEARDRIVE_CONFIG_PATH") {
                Some(PathBuf::from(path))
            } else {
                if let Some(dir) = dirs::config_dir() {
                    let file = dir.join("speardrive").join("config.yaml");
                    if file.exists() {
                        Some(file)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let mut settings = cconfig::Config::builder();
        if let Some(config_path) = config_path {
            settings = settings.add_source(cconfig::File::new(
                config_path.to_str().ok_or_else(|| Error::ConfigFile)?,
                cconfig::FileFormat::Yaml,
            ));
        }
        settings = settings.add_source(
            cconfig::Environment::with_prefix("SPEARDRIVE")
                .translate_key(TranslationType::Kebab)
                .separator("__"),
        );

        let built_config = settings.build()?;
        let config = built_config.try_deserialize();
        let config = config?;

        if opt.dump_config {
            log::info!("{}", serde_yaml::to_string(&config)?);
        }

        Ok(config)
    }

    async fn run(&mut self) -> Result<(), Error> {
        let addr = match self.config.listen_addr.to_socket_addrs() {
            Ok(addr) => addr.collect::<Vec<_>>().pop().unwrap(),
            Err(err) => return Err(Error::InvalidAddress(format!("{:?}", err))),
        };

        let config = Arc::new(self.config.clone());
        let make_svc = make_service_fn(move |_conn| {
            let config = config.clone();
            let service_handler = move |req| service_handle_wrapper(config.clone(), req);
            async move { Ok::<_, Infallible>(service_fn(service_handler)) }
        });
        let bound = hyper::Server::bind(&addr);

        log::info!("waiting for requests");

        let server = bound.serve(make_svc);

        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }

        Ok(())
    }
}

fn main_wrap() -> Result<(), Error> {
    let opt = CommandArgs::from_args();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(3)
        .build()?
        .block_on(async {
            match Main::new(&opt).await {
                Err(err) => Err(err),
                Ok(mut main) => main.run().await,
            }
        })?;

    Ok(())
}

fn main() {
    match main_wrap() {
        Ok(()) => {}
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(-1);
        }
    }
}
