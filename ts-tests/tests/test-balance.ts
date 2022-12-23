import { expect } from "chai";
import { step } from "mocha-steps";

import { GENESIS_ACCOUNT, GENESIS_ACCOUNT_PRIVATE_KEY, EXISTENTIAL_DEPOSIT } from "./config";
import { createAndFinalizeBlock, describeWithFrontier, customRequest } from "./util";

describeWithFrontier("Frontier RPC (Balance)", (context) => {
	const TEST_ACCOUNT = "0x1111111111111111111111111111111111111111";
	const GENESIS_ACCOUNT_BALANCE = "34028236692093846346337460743176821095500000000";

	step("genesis balance is setup correctly", async function () {
		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(GENESIS_ACCOUNT_BALANCE);
	});

	step("balance to be not updated after transfer", async function () {
		await createAndFinalizeBlock(context.web3);
		this.timeout(15000);

		const value = "0x200"; // Low value(< 100000000) will transfer 0
		const gasPrice = "0x3B9ACA00"; // BASE_FEE=1000000000
		const tx = await context.web3.eth.accounts.signTransaction(
			{
				from: GENESIS_ACCOUNT,
				to: TEST_ACCOUNT,
				value: value,
				gasPrice: gasPrice,
				gas: "0x100000",
			},
			GENESIS_ACCOUNT_PRIVATE_KEY
		);
		await customRequest(context.web3, "eth_sendRawTransaction", [tx.rawTransaction]);

		// GENESIS_ACCOUNT_BALANCE - (21000 * gasPrice)
		const expectedGenesisBalance = (
			BigInt(GENESIS_ACCOUNT_BALANCE) -
			BigInt(21000) * BigInt(gasPrice)
		).toString();
		const expectedTestBalance = "0";

		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT, "pending")).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT, "pending")).to.equal(expectedTestBalance);

		await createAndFinalizeBlock(context.web3);

		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT)).to.equal(expectedTestBalance);
	});

	step("balance to be updated after transfer", async function () {
		await createAndFinalizeBlock(context.web3);
		this.timeout(15000);

		const value = "0xBEBC20000"; // Must be higher than ExistentialDeposit 500 (512 after psc_shrink(51200000000))
		const gasPrice = "0x3B9ACA00"; // BASE_FEE=1000000000
		const tx = await context.web3.eth.accounts.signTransaction(
			{
				from: GENESIS_ACCOUNT,
				to: TEST_ACCOUNT,
				value: value,
				gasPrice: gasPrice,
				gas: "0x100000",
			},
			GENESIS_ACCOUNT_PRIVATE_KEY
		);
		await customRequest(context.web3, "eth_sendRawTransaction", [tx.rawTransaction]);

		// GENESIS_ACCOUNT_BALANCE - (21000 * gasPrice) - (21000 * gasPrice) - value
		const expectedGenesisBalance = (
			BigInt(GENESIS_ACCOUNT_BALANCE) -
			// Because of the previous transaction
			BigInt(21000) * BigInt(gasPrice) -
			BigInt(21000) * BigInt(gasPrice) -
			BigInt(value)
		).toString();
		const expectedTestBalance = (Number(value) - EXISTENTIAL_DEPOSIT*100000000).toString();

		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT, "pending")).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT, "pending")).to.equal(expectedTestBalance);

		await createAndFinalizeBlock(context.web3);

		expect(await context.web3.eth.getBalance(GENESIS_ACCOUNT)).to.equal(expectedGenesisBalance);
		expect(await context.web3.eth.getBalance(TEST_ACCOUNT)).to.equal(expectedTestBalance);
	});
});
