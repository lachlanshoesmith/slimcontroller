<h1 align="center">slimcontroller 🧙</h1>
<p align="center">a stupidly simple URL shortener. or lengthener. redirectioner.
</p>
<p align="center">
<img width="300" src="/screenshot.jpg?raw=true">
</p>

## running

to run slimcontroller, you'll need to have a redis server running somewhere. spinning up a redis stack server locally is easy:

`docker run -d --name redis-stack-server -p 6379:6379 redis/redis-stack-server:latest`

note that redis is not persistent by default, so all shorthands created will be wiped when the server is restarted. it's up to you on how you handle this. [more info here](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)

### docker

1. `docker build -t slimcontroller .`
2. `docker run -p <SERVER_PORT>:<SERVER_PORT> -e REDIS_URL=<REDIS_URL> slimcontroller`

### from source

1. `cargo build --release`
2. `./target/release/slimcontroller <SERVER_PORT> <REDIS_URL>`
3. the frontend can then be accessed via `localhost:SERVER_PORT`. if you want to host a different `index.html`, or it's in a different location, use the `--index <FILE_PATH>` flag. the same may be said for `--admin`.

### mandatory arguments

any of the following may be supplied as environment variables.

- `SERVER_PORT` can be any `u16`
- `REDIS_URL` can be a URL (`String`) or port (`u16`)
  - if a port alone is provided, it's assumed your server's running locally
  - a valid `REDIS_URL` might be `127.0.0.1:6379` if you ran [the above command](#running)

you should specify an `--admin-password`. see [authentication](#authentication) for more information.

## deployment

i recommend following the above docker instructions. don't forget to set the `--server-hostname` (may be an environment variable `SERVER_HOSTNAME`).

## authentication

if you'd like, you can make individual set/delete operations require a password. just pass in the `--password <PASSWORD>` flag.

you should provide an `--admin-password` when running slimcontroller. if you don't provide one, your admin password will be the same as your `--password`. if you provide neither, you will not be able to access the `/admin` panel at all.

so long as you have an admin password of _some_ variety, you can access the `/admin` panel.
