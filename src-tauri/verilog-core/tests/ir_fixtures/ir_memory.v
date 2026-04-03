// Simple memory and indexed access: tests array read/write lowering.
module ir_memory(input clk,
                 input we,
                 input [1:0] addr,
                 input [7:0] wdata,
                 output [7:0] rdata);
  reg [7:0] mem [0:3];

  always @(posedge clk) begin
    if (we)
      mem[addr] <= wdata;
  end

  assign rdata = mem[addr];
endmodule

