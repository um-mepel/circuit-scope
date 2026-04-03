// Testbench for counter.v — intended for Icarus Verilog (iverilog + vvp).
//
// The DUT generates its own `clk` and holds `rst`; this TB only monitors.
//
// Run from this directory:
//   iverilog -o sim counter_tb.v counter.v && vvp sim
// Produces: counter_tb.vcd
`timescale 1ns / 1ns

module counter_tb;
  wire [3:0] count;

  counter dut (
      .count(count)
  );

  initial begin
    $dumpfile("counter_tb.vcd");
    $dumpvars(0, dut);
  end

  initial begin
    #2000;
    $finish;
  end
endmodule
