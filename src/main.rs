use std::{convert::Infallible, net::SocketAddr, path::PathBuf, sync::Arc, collections::{VecDeque, HashMap}, str::FromStr};

use cmdline::CommandArgs;
use error::Error;
use fs2::FileExt;
use gitlab::{api::AsyncQuery, AsyncGitlab, GitlabBuilder};
use hyper::{
    service::{make_service_fn, service_fn},
    Body, Request, Response, Uri, http::uri::PathAndQuery,
};
use regex::Regex;
use structopt::StructOpt;

mod artifacts;
mod cmdline;
mod config;
mod error;
mod logging;
mod util;

use crate::config::{Config, GitlabJobArtifacts};

struct Main {
    config: Config,
}

#[derive(Debug)]
struct Plan {
    jobs: Vec<JobArtifact>,
    sub_uri: String,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    RPM,
}

#[derive(Debug)]
struct JobArtifact {
    name: String,
    project: String,
    job_id: u64,
}

impl Plan {
    fn to_composite_path(&self) -> String {
        use sha2::{Digest, Sha256};

        let mut hasher = Sha256::new();
        let rep = format!("{:?}" ,self);
        hasher.update(rep.as_bytes());
        let result = hasher.finalize();

        return format!("{}", hex::encode(result));
    }

    fn from_uri(uri: &str, config: &Arc<Config>) -> Result<Plan, Error> {
        let mut jobs = vec![];

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

            for gl in config.gitlabs.iter() {
                if &gl.name != prefix {
                    continue;
                }

                if let Some(job_id) = parts.pop_back() {
                    let project = parts.into_iter().collect::<Vec<_>>().join("/");

                    if !RE.is_match(&project) {
                        return Err(Error::PlanParse(
                                format!("{} invalid project name", project)));
                    }

                    jobs.push(JobArtifact {
                        name: prefix.to_owned(),
                        project,
                        job_id: job_id.parse()?,
                    })
                }
                break;
            }
        }

        Ok(Plan { jobs, sub_uri, kind })
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

    async fn get(&mut self, gpipe: &GitlabJobArtifacts) -> Result<&mut AsyncGitlab, Error> {
        if !self.gitlab_clients.contains_key(&gpipe.name) {
            let builder = GitlabBuilder::new(&gpipe.hostname, &gpipe.api_key);
            let gitlab = builder.build_async().await?;

            self.gitlab_clients.insert(gpipe.name.clone(), gitlab);
        }

        Ok(self.gitlab_clients.get_mut(&gpipe.name).unwrap())
    }
}

async fn service_handle(config: Arc<Config>, req: Request<Body>) -> Result<Response<Body>, Error> {
    let uri = req.uri().to_string();
    log::info!("request: {}", uri);

    let plan = Plan::from_uri(&uri, &config)?;
    log::info!("request: plan - {:?}", plan);

    let gpipe = config.gitlabs.first().unwrap();
    let mut gitlab = ClientCache::new();

    for job in plan.jobs.iter() {
        let mut gpipe = None;

        for gitlab_client in config.gitlabs.iter() {
            if gitlab_client.name == job.name {
                gpipe = Some(gitlab_client);
                break;
            }
        }

        if let Some(gpipe) = gpipe {
            let project_path = gpipe.local_cache.join(&job.name).join(&job.project);
            let lock = project_path.join(format!("lock"));
            let path_tmp = project_path.join(format!("{}.tmp", job.job_id));
            let path = project_path.join(format!("{}", job.job_id));

            if path.exists() {
                log::info!("request: {}: artifacts exist", uri);
                continue;
            }

            cache_job_artifacts(project_path, lock, path_tmp, job, gpipe, &uri, &mut gitlab, path).await?;
        }
    }

    // Create composite directory
    let lock = config.composites_cache.join(format!("lock"));
    let node_name = plan.to_composite_path();
    let composite_path = config.composites_cache.join(&node_name);
    let path_tmp = config
        .composites_cache
        .join(format!("{}.tmp", node_name));

    if !composite_path.exists() {
        log::info!("request: {}: creating composite path", uri);

        let lockfile = std::fs::File::create(&lock)?;
        lockfile.lock_exclusive()?;

        let _ = std::fs::remove_dir_all(&path_tmp);
        std::fs::create_dir_all(&path_tmp)?;

        for (idx, job) in plan.jobs.iter().enumerate() {
            let project_path = gpipe.local_cache.join(&job.project);
            let cache_path = project_path.join(format!("{}", job.job_id));
            let cache_path = cache_path.display();
            let path_tmp = path_tmp.display();

            util::bash(format!("cp -al {cache_path} {path_tmp}/{idx}"))?;
        }

        std::fs::write(path_tmp.join("url.txt"), node_name)?;

        match plan.kind {
            Kind::RPM => {
                let path_tmp = path_tmp.display();
                util::bash(format!("createrepo {path_tmp}"))?;
            }
        }

        std::fs::rename(path_tmp, &composite_path)?;
    }

    let static_ = hyper_staticfile::Static::new(composite_path);

    let mut req = req;
    let mut parts = req.uri().clone().into_parts();
    if let Some(p) = &mut parts.path_and_query {
        *p = PathAndQuery::from_str(&plan.sub_uri).unwrap();
    }
    *req.uri_mut() = Uri::from_parts(parts)?;

    Ok(static_.serve(req).await?)
}

async fn cache_job_artifacts(
    project_path: PathBuf,
    lock: PathBuf,
    path_tmp: PathBuf,
    job: &JobArtifact,
    gpipe: &GitlabJobArtifacts,
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
        .query_async(gitlab.get(gpipe).await?)
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

impl Main {
    async fn new(opt: &CommandArgs) -> Result<Self, Error> {
        logging::activate(&opt.logging, logging::empty_filter)?;

        match opt.cmd {
            cmdline::Command::ExampleConf => {
                println!(
                    "{}",
                    serde_yaml::to_string(&Config {
                        composites_cache: PathBuf::from("/storage/for/repo-composites"),
                        gitlabs: vec![GitlabJobArtifacts {
                            name: "myserver".to_owned(),
                            api_key: "SomeAPIKEYObtainedFromGitlab".to_owned(),
                            hostname: "git.myserver.com".to_owned(),
                            local_cache: PathBuf::from("/storage/for/cached-job-artifacts"),
                        }]
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

        let mut settings = cconfig::Config::default();
        if let Some(config_path) = config_path {
            settings
                .merge(cconfig::File::new(
                        config_path.to_str().ok_or_else(|| Error::ConfigFile)?,
                        cconfig::FileFormat::Yaml,
                ))?;
        }
        settings.merge(cconfig::Environment::with_prefix("SPEARDRIVE_CONF_"))?;

        let config = settings.try_into::<Config>()?;

        if opt.dump_config {
            log::info!("{}", serde_yaml::to_string(&config)?);
        }

        Ok(config)
    }

    async fn run(&mut self) -> Result<(), Error> {
        let addr = SocketAddr::from(([127, 0, 0, 1], 4444));

        let config = Arc::new(self.config.clone());
        let make_svc = make_service_fn(move |_conn| {
            let config = config.clone();
            let service_handler = move |req| service_handle(config.clone(), req);
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
