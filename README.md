## speardrive - Dynamically create package repositories from CI job artifacts

A small web server that creates and serves read-only package repositories
based on a plan from an accessed URL.

For example:

```
wget http://127.0.0.1:4444/myserver/foo/323/-/myserver/bar/111/-/rpm/repodata/repomd.xml
```

Immediately serve an RPM repo combining the RPM artifact outputs of job 323 of
project `foo` and job 111 of project `bar` from configured Gitlab server
`myserver`.


Highlights:

* Supports Gitlab CI job artifacts.
* Supports RPM repositories.
* Downloads artifacts and caches them locally per job.
* Caches the combination of requested repositories.


### Installation

Install after Rust toolchain with `cargo install --path .`


### Configuration

```
composites-cache: /storage/for/repo-composites
gitlabs:
  'myserver':
     api-key: SomeAPIKEYObtainedFromGitlab
     hostname: git.myserver.com
     local-cache: /storage/for/cached-job-artifacts
```

## Command line

```
speardrive --config-path <pathname>
```
