#!/bin/bash

set -e

local() {
    EXENAME=speardrive
    dest=bin/${EXENAME}

    while [[ $# != 0 ]] ; do
	if [[ "$1" == "--dest" ]] ; then
	    shift
	    dest=$1
	    shift
	    continue
	fi
	break
    done

    T=/tmp/$USER/rust/targets/$(pwd)/target

    mkdir -p $T
    cargo build --release --target-dir ${T}

    mkdir -p bin/
    rm -f ${dest}
    cp $T/release/${EXENAME} ${dest}
}

_docker() {
    set -eu
    set -o pipefail
    set +o posix

    VERSION=$(grep '^version =' Cargo.toml | awk -F'"' '{print $2}')
    docker build -t alonid/speardrive:${VERSION} .
    # docker push alonid/speardrive:${VERSION}
}

if [[ "$1" == "" ]] ; then
    echo "build.sh docker/local"
else
    case $1 in
	local) "$@" ;;
	docker) shift; _docker "$@" ;;
	*) echo invalid command $1; exit -1;;
    esac
fi
