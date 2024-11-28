# slimcontroller

a stupidly simple URL redirection solution

## running

to run slimcontroller, you'll need to have a redis server running somewhere. spinning up a redis stack server locally is easy:

`docker run -d --name redis-stack-server -p 6379:6379 redis/redis-stack-server:latest`

note that redis is not persistent by default, so all shorthands created will be wiped when the server is restarted. it's up to you on how you handle this. [more info here](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)

### docker

1. `docker build -t slimcontroller .`
2. `docker run -p <SERVER_PORT>:<SERVER_PORT> -e PASSWORD=<PASSWORD> -e REDIS_URL=<REDIS_URL> -e ADMIN_PASSWORD=<ADMIN_PASSWORD> slimcontroller`

### from source

1. `cargo build --release`
2. `./target/release/slimcontroller <SERVER_PORT> <REDIS_URL>`
3. the frontend can then be accessed via `localhost:SERVER_PORT`. if you want to host a different `index.html`, or it's in a different location, use the `-f <FILE_PATH>` flag.

### mandatory arguments

any of the following may be supplied as environment variables.

- `SERVER_PORT` can be any `u16`,
- `REDIS_URL` might be `127.0.0.1:6379` if you ran the above command, and

updating the `--admin-password` is also strongly advisable. see [authentication](#authentication) for more information.

## deployment

i recommend following the above docker instructions. don't forget to set the `--server-hostname` (may be an environment variable `SERVER_HOSTNAME`).

## authentication

if you'd like, you can make individual set/delete operations require a password. just pass in the `--password <PASSWORD>` flag.

you should provide an `--admin-password` when running slimcontroller. with this you can access the `/admin` panel. if you don't, it will be the same as the `--password`. if you provide neither, you will not be able to access the `/admin` panel at all.
