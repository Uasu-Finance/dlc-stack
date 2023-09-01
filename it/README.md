# Integration Test

This is a simple NodeJs application that is using the `dlc-stack` to go through a simple DLC flow.

It compiles and wraps the `wasm-wallet` module to be used in a NodeJs environment.

## How to run

There are several ways to run the integration test, and it also runs automatically on PRs to our `dev` and `main` branches.

> Take a look inside the `.env.template` and the `./config.js` files to see what environment variables are needed/default. If you want to change them, create a `.env` file in this folder, and set the variables there.

### 1. As a standalone NodeJs application

> Make sure you have the necessary applications running: attestor, wallet, storage-api, and postgres (or set the `.env` such that it will use remote ones)

In this folder, run:

```bash
npm install
npm run build
npm run start
```

### 2. Docker Compose through `make`

In the root of the repository, run:

```bash
make integration-test
```

This will build and start all the necessary docker images, and run the integration test as the last step in a container. If it's the first time you run this, it will take a while to build all the images.

If you update a module, you can run the following to rebuild the image:

```bash
docker compose build <module-name>
```
