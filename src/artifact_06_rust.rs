// Compute Budget configuration snippet for client-side bundle:
import { ComputeBudgetProgram } from "@solana/web3.js";

const modifyComputeUnits = ComputeBudgetProgram.setComputeUnitLimit({ 
  units: 35_000 
});
const addPriorityFee = ComputeBudgetProgram.setComputeUnitPrice({ 
  microLamports: 1_000 
});

tx.add(modifyComputeUnits).add(addPriorityFee).add(instruction);