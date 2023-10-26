import express from 'express';

export const localhostOnly = (req: express.Request, res: express.Response, next: express.NextFunction) => {
    const remoteAddress = req.socket.remoteAddress;

    const isLocalhost = req.hostname === 'localhost';
    const isDockerNetwork =
        remoteAddress &&
        (remoteAddress.startsWith('172.') ||
            remoteAddress.startsWith('172:') ||
            remoteAddress.startsWith('::ffff:172.'));

    if (isLocalhost || isDockerNetwork) {
        next();
    } else {
        res.status(403).send('Forbidden');
    }
};
