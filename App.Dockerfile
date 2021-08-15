FROM rust:1.54

WORKDIR /app

COPY ./batch ./batch
COPY ./rust ./rust
COPY ./WebContent ./WebContent

RUN apt update

# Setup autotrader system
RUN apt install -y default-libmysqlclient-dev

WORKDIR /app/rust
RUN cargo build --all
WORKDIR /app

# Setup cron
RUN apt install -y cron
ADD ./docker/crontab /var/spool/crontab/root
RUN crontab /var/spool/crontab/root

RUN crontab -l > tmpcron
RUN echo >> tmpcron
RUN echo "*/5 * * * * /app/batch/scraping.sh" >> tmpcron
RUN echo "0 12 * * 0 /app/batch/log_archive.sh" >> tmpcron
RUN crontab tmpcron

RUN service cron restart

RUN mkdir /app/log

EXPOSE 8080
