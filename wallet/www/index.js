import { JsDLCInterface } from "dlc_protocol_wallet";


async function setup() {
    const dlc_man = await JsDLCInterface.new();
    console.log(dlc_man.send_options_to_js());
    dlc_man.receive_offer(
        test_offer
    )
}

const test_offer = {
    "protocolVersion": 1,
    "contractFlags": 0,
    "chainHash": "06226e46111a0b59caaf126043eb5bbf28c34f3a5e332a1fc7b2b73cf188910f",
    "temporaryContractId": "b4129523d53598de72d8c3023e52ccd2201a3e79c3b0d21b25c934c2d54bab52",
    "contractInfo": {
        "singleContractInfo": {
            "totalCollateral": 10000,
            "contractInfo": {
                "contractDescriptor": {
                    "numericOutcomeContractDescriptor": {
                        "numDigits": 14,
                        "payoutFunction": {
                            "payoutFunctionPieces": [
                                {
                                    "endPoint": {
                                        "eventOutcome": 0,
                                        "outcomePayout": 0,
                                        "extraPrecision": 0
                                    },
                                    "payoutCurvePiece": {
                                        "polynomialPayoutCurvePiece": {
                                            "payoutPoints": []
                                        }
                                    }
                                },
                                {
                                    "endPoint": {
                                        "eventOutcome": 1,
                                        "outcomePayout": 10000,
                                        "extraPrecision": 0
                                    },
                                    "payoutCurvePiece": {
                                        "polynomialPayoutCurvePiece": {
                                            "payoutPoints": []
                                        }
                                    }
                                }
                            ],
                            "lastEndpoint": {
                                "eventOutcome": 16383,
                                "outcomePayout": 10000,
                                "extraPrecision": 0
                            }
                        },
                        "roundingIntervals": {
                            "intervals": [
                                {
                                    "beginInterval": 0,
                                    "roundingMod": 1
                                }
                            ]
                        }
                    }
                },
                "oracleInfo": {
                    "single": {
                        "oracleAnnouncement": {
                            "announcementSignature": "dd422c42fc3355a76c614ed82ac5599d6332784522531b292211789364dc7a43202fc27015ddcf3ffe3037b094bcd5ff5dce727d27a807fdd4e90dd4976540f4",
                            "oraclePublicKey": "57c75f44e4dde2a17c0da725c160267e3307d771a33685dcd67ec79c7614722c",
                            "oracleEvent": {
                                "oracleNonces": [
                                    "8918249a618480a094c73c316a40df2694544b1dcf06aa53889fdb2c4a8b69a5",
                                    "60bce330a8ef75b1673f83c9dc15026f9ace6d2f29bef9266b4e3fc6b8d28656",
                                    "cc1e31967467429d96d5630ed1e77b44e451addf7ce103dfa4f4913bc875c607",
                                    "f6ea9b082a0f9e80c1517e617b950801ea367bddaae5671348090c3c1cd3f3b5",
                                    "64b8ca80ccb1546711ffe4f15575cd6ee301dcfce642ad71f2249d3d1012db4b",
                                    "1b4d740974d07d472dc963a35a2953061cb8684d719233169cf662cd837c8be7",
                                    "b491ca977a2cb8073f84b509ab410e426f57db0e1c53e2df375549eb05e4b420",
                                    "a3ade832b5e5300b3adbaa90b88c1260ea43b753a59d7a693985cbb4cdafc0c4",
                                    "7a96cc4b8c9eac3bb833cd3cb24b9167f83234374d0d519b2c722220d2881cfc",
                                    "a99cc1e79e1be4d8f262bddf4dc360e7411d6a912465e88f4eced9f27eb4e48e",
                                    "89b9548852b90f429d8b9e40244deb823b3b2295e730703527d4aa7cf318dc64",
                                    "f4d7faad7aebe4dc6f14b069f8da76223a61345459dec023b68a8c8a110135df",
                                    "64370b93d4cd420bba7e4d83a66d4b88eecb146cf9dd0c6d9a28c6e53f3e9ab6",
                                    "6fdc9cf363194fe9c01f1242403065018aac57b186d4b8543aef917e6b1d91da"
                                ],
                                "eventMaturityEpoch": 1696772880,
                                "eventDescriptor": {
                                    "digitDecompositionEvent": {
                                        "base": 2,
                                        "isSigned": false,
                                        "unit": "BTCUSD",
                                        "precision": 0,
                                        "nbDigits": 14
                                    }
                                },
                                "eventId": "abc123"
                            }
                        }
                    }
                }
            }
        }
    },
    "fundingPubkey": "03221a633121aa681f31d7751728ae69db9c7f06bf90bea7e0562358a7e519a971",
    "payoutSpk": "001450f2e8fe80c50537ec7e2fe9893c1c0a893d7237",
    "payoutSerialId": 16753909255317621797n,
    "offerCollateral": 0,
    "fundingInputs": [
        {
            "inputSerialId": 1503915407986778031n,
            "prevTx": "010000000122f92d3bc4864c7ee9fc4671a80dfbebb930daec20420b855ad5272b455e2e5d010000006b483045022100fb79eb5ce89bf2f8de4b60faf09505430708f98147b3d403eca250af030fa7d4022027a3bfb006d7496ff99e5d4a251610b3b95bdb4513d9edb82fcb2ace6f68de7f012102add319140c528a8955d76d4afe32c4d3143fea57ea353a31ce793cffb77ef861fdffffff0200e1f505000000001600146430186e1b81e4badb6b364b98ad6d03e1297f2d70dc3c71000000001976a9142b19bade75a48768a5ffc142a86490303a95f41388ac00000000",
            "prevTxVout": 0,
            "sequence": 4294967295,
            "maxWitnessLen": 107,
            "redeemScript": ""
        }
    ],
    "changeSpk": "001493c47b7e5409a9c096ffdb655c9b62d8a7763742",
    "changeSerialId": 17274976141736825804n,
    "fundOutputSerialId": 4648695136085557376n,
    "feeRatePerVb": 1,
    "cetLocktime": 1682125635,
    "refundLocktime": 1697377680
};


setup();
