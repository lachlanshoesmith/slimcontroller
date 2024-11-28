# slimcontroller

a stupidly simple URL redirection solution

## running

the frontend can be run by serving index.html. note that you will need to update the backend address; this is stored in the `BACKEND_URL` constant at the top of the file.

to run the backend, you'll need to have a redis server running somewhere. spinning up a redis stack server is easy:

`docker run -d --name redis-stack-server -p 6379:6379 redis/redis-stack-server:latest`

note that redis is not persistent by default, so all shorthands created will be wiped when the server is restarted. it's up to you on how you handle this. [more info here](https://redis.io/docs/latest/operate/oss_and_stack/management/persistence/)

then, `cargo build --release`, and run `./target/release/slimcontroller <SERVER_PORT> <REDIS_URL>`, where `SERVER_PORT` can be any `u8`, and `REDIS_URL` might be `127.0.0.1:6379` if you ran the above command.
