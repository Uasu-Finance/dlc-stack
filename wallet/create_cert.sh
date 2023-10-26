mkdir ./wallet-blockchain-interface/dist/.cert

openssl req -subj '/CN=' -new -newkey rsa:2048 -sha256 -days 365 -nodes -x509 -keyout server.key -out server.crt

mv server.crt ./wallet-blockchain-interface/dist/.cert
mv server.key ./wallet-blockchain-interface/dist/.cert
