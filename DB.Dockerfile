FROM mysql:5.7
ENV MYSQL_ROOT_PASSWORD=autotrader

COPY ./docker/docker-entrypoint-initdb.d /docker-entrypoint-initdb.d

EXPOSE 3306
