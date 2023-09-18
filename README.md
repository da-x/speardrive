## speardrive - Dynamically create package repositories from CI job artifacts

A small web server that creates and serves read-only package repositories
based on a plan from an accessed URL.

For example:

```
wget http://127.0.0.1:3200/myserver/foo/323/-/myserver/bar/111/-/rpm/repodata/repomd.xml
```

Immediately serve an RPM repo combining the RPM artifact outputs of job 323 of
project `foo` and job 111 of project `bar` from configured Gitlab server
`myserver`.


Highlights:

* Supports Gitlab CI job artifacts.
* Supports locally available artifacts.
* Supports remotely available from URLs.
* Supports generating RPM repositories.
* Downloads artifacts and caches them locally per job.
* Caches the combination of requested repositories.


## Command line

```
speardrive --config-path <pathname>
```

### URL format

Repos are created when their URLs are accessed, and the URLs define the read-only
content of the repos.

`<source-spec-a>/-/<source-spec-b>/-/.../<repo-type>`

Where `<source-spec>` can be:

* `<gitlab-source-name>/<project-id>/<job-id>`
* `<local-source-name>/<dirname>`
* `<remote-static-name>/<dirname>`

And `<repo-type>` can be:

* `rpm` - Use `createrepo_c` to create local repositories


### Configuration

Two types of sources are supported:

 * Gitlab job artifacts
 * A flat local directory

```
listen-addr: 127.0.0.1:3200
composites-cache: /storage/for/repo-composites
local-cache: /storage/for/cached-job-artifacts
gitlabs:
  'myserver':
     api-key: SomeAPIKEYObtainedFromGitlab
     hostname: git.myserver.com
local-source:
  local:
    root: /home/user/builds
remote-static:
  remote:
    base-url: https://some_static_site/suburl
```

## Static remotes

For each `<remote-static-name>/<dirname>`, we will use the `<base_url>/<dirname>/list.txt` as
the list of files to download under `<base_url>/<dirname>`. This list can be generated
using `find -type f`.


## Deployment example

Prebuilt images are available from dockerhub.

Example deployment script:
```
# Cleanup previous instance
docker rm -f speardrive 2>/dev/null

# Start new instance
docker run \
        --name speardrive \
        --network host \
        --user $(id -u):$(id -g) \
        --log-driver local --log-opt max-size=10m \
        -v /storage/speardrive:/storage/speardrive \
        -v $(pwd)/speardrive.yaml:/dist/config.yaml \
        -d "$@" \
        alonid/speardrive:0.1.6 /dist/speardrive \
        -c /dist/config.yaml  \
        serve
```

## Example nginx location config

The following assumes a spawned speardrive instance that listens on
`127.0.0.1:3200`. You can serve it under a prefix using your existing nginx
instance. For example:

```
location  /speardrive {
    rewrite /speardrive/(.*) /$1  break;
    proxy_pass         http://localhost:3200;
    proxy_redirect     off;
    proxy_set_header   Host $host;
}
```


### Building from source

If you don't want to use the prebuilt docker image, you can build speardrive by
yourself using Rust. With the Rust toolchain installed, run `cargo install
--path .`
