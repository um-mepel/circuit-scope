`timescale 1ns/1ns
// Stripped controller pattern from FourFuncCalc: `always @*` case + `<=` to next state,
// plus `always @(posedge clk)` state update. No child modules — isolates parse/sim of FSM shell.
module p7_case_nba_fsm_tb;
  reg clk;
  reg clear;
  reg equals;
  reg add;

  reg [2:0] X;
  reg [2:0] X_Next;

  localparam XInit = 3'd0;
  localparam XClear = 3'd1;
  localparam XLoadA = 3'd2;
  localparam XAdd = 3'd3;
  localparam XDisp = 3'd4;

  always @*
    case (X)
      XInit:
        if (clear)
          X_Next <= XInit;
        else if (equals)
          X_Next <= XLoadA;
        else if (add)
          X_Next <= XAdd;
        else
          X_Next <= XInit;
      XClear:
        X_Next <= XInit;
      XAdd:
        if (equals)
          X_Next <= XDisp;
        else
          X_Next <= XAdd;
      XLoadA:
        X_Next <= XDisp;
      XDisp:
        if (add)
          X_Next <= XAdd;
        else
          X_Next <= XDisp;
      default:
        X_Next <= XClear;
    endcase

  always @(posedge clk)
    if (clear)
      X <= XClear;
    else
      X <= X_Next;

  initial begin
    clk = 0;
    clear = 0;
    equals = 0;
    add = 0;
  end

  // Match TestBench7-style clock period (20 ns): toggle every 10.
  always #10 clk = ~clk;

  initial begin
    X = XClear;
    #5;
    clear = 1;
    #20;
    clear = 0;
    #20;
    equals = 1;
    #20;
    equals = 0;
    #20;
    add = 1;
    #20;
    add = 0;
    #20;
    equals = 1;
    #20;
    equals = 0;
    #200;
  end
endmodule
