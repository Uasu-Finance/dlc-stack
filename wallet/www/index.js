import { JsDLCInterface } from "dlc_protocol_wallet";

async function setup() {
    const dlc_man = await JsDLCInterface.new();
    console.log(dlc_man.send_options_to_js());
    dlc_man.receive_offer
}

setup();
