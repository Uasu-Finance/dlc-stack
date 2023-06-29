# Node.js Dockerfile with TypeScript
# Use node base image with alpine as it's smaller and quicker to pull
FROM node:16-alpine

# Create app directory
WORKDIR /usr/src/app

# Install app dependencies
# A wildcard is used to ensure both package.json AND package-lock.json are copied
COPY package*.json ./

# If you are using npm
RUN npm install

# Bundle app source inside Docker image
COPY . .

# Build the TypeScript app
RUN npm run build

# Expose the app on the specified port
EXPOSE 3000

# Define the command to run the app using PM2
CMD [ "node", "dist/src/index.js" ]
