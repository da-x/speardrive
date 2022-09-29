# Build
FROM rockylinux:8.5.20220308 as builder

RUN echo fastestmirror=True >> /etc/dnf/dnf.conf
RUN yum -y install epel-release
RUN yum -y install gcc openssl-devel
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

RUN source $HOME/.cargo/env && (cargo search speardrive > /dev/null || false)

COPY . /work
RUN source $HOME/.cargo/env && cd /work && ./build.sh local
RUN dnf install -y unzip createrepo_c

# Run-time image
FROM rockylinux:8.5.20220308
COPY --from=builder /work/bin/speardrive /dist/speardrive
CMD ["/dist/speardrive"]
