# Build blockchain-interface NodeJS project
FROM node:18-alpine AS BI-build

WORKDIR /app/blockchain-interface

COPY ./wallet/wallet-blockchain-interface/package.json ./package.json
RUN npm install
COPY ./wallet/wallet-blockchain-interface/src ./src
COPY ./wallet/wallet-blockchain-interface/tsconfig.json ./tsconfig.json
RUN npm run build
EXPOSE 3000
EXPOSE 3003
CMD [ "node", "dist/src/index.js" ]
