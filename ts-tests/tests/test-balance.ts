import { expect } from "chai";
import { step } from "mocha-steps";

import { createAndFinalizeBlock, describeWithFrontier, customRequest } from "./util";

describeWithFrontier("Frontier RPC (Balance)", (context) => {
	const GENESIS_ACCOUNT = "0x6be02d1d3665660d22ff9624b7be0551ee1ac91b";
	const GENESIS_ACCOUNT_BALANCE = "3402823669209384634633746074317682109550000000000";
	const GENESIS_ACCOUNT_PRIVATE_KEY = "0x99B3C12287537E38C90A9219D4CB074A89A16E9CDB20BF85728EBD97C343E342";
	const TEST_ACCOUNT = "0x1111111111111111111111111111111111111111";

	step("genesis balance is setup correctly", async function () {
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(GENESIS_ACCOUNT_BALANCE);
	});

	step("balance to be updated after transfer with low value", async function () {
		this.timeout(15000);

		const tx = await context.web3.eth.accounts.signTransaction({
			from: GENESIS_ACCOUNT,
			to: TEST_ACCOUNT,
			value: "0x200", // Low value(< 10000000000) will transfer fail
			gasPrice: "0x2540be400", // 10000000000
			gas: "0x100000",
		}, GENESIS_ACCOUNT_PRIVATE_KEY);
		await customRequest(context.web3, "eth_sendRawTransaction", [tx.rawTransaction]);
		const expectedGenesisBalance = "3402823669209384634633746074317681899550000000000";
		const expectedTestBalance = "0";
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT, "pending")).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT, "pending")).to.equal(expectedTestBalance);
		await createAndFinalizeBlock(context.web3);
		// 3402823669209384634633746074317681899550000000000 - (21000 * 10000000000);
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT)).to.equal(expectedTestBalance);
	});

	step("balance to be updated after transfer", async function () {
		this.timeout(15000);

		const tx = await context.web3.eth.accounts.signTransaction({
			from: GENESIS_ACCOUNT,
			to: TEST_ACCOUNT,
			value: "0x4a817c80000", // Must me higher than ExistentialDeposit (512 after chainx shrink 10000000000)
			gasPrice: "0x2540be400", // 10000000000
			gas: "0x100000",
		}, GENESIS_ACCOUNT_PRIVATE_KEY);
		await customRequest(context.web3, "eth_sendRawTransaction", [tx.rawTransaction]);
		const expectedGenesisBalance = "3402823669209384634633746074317681684430000000000";
		const expectedTestBalance = "120000000000";
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT, "pending")).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT, "pending")).to.equal(expectedTestBalance);
		await createAndFinalizeBlock(context.web3);
		// 3402823669209384634633746074317682109550000000000 - (21000 * 10000000000) * 2 - 5120000000000;
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT)).to.equal(expectedTestBalance);
	});
});
