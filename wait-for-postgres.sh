#!/bin/sh
# Modified from https://docs.docker.com/compose/startup-order/

set -e

host="$1"
port="$2"
shift 2
times=0

while [ $times -lt 2 ]; do
    ! PGPASSWORD=$POSTGRES_PASSWORD psql -h $host -p $port > /dev/null 2>&1

    if [ $? -eq 0 ]; then
        times=$((times + 1))
    fi

    >&2 echo "Postgres is unavailable - sleeping"
    sleep 2
done

echo "Postgres is up - executing command $cmd"
./vertex_server "0.0.0.0:443"
echo "Command executed"
