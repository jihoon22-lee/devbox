import { describe, expect, it } from "vitest";
import { placeRootMenu, placeSubmenu } from "./position";

describe("placeRootMenu", () => {
  it("uses the pointer as top-left when the menu fits", () => {
    expect(
      placeRootMenu(
        { x: 100, y: 80 },
        { width: 180, height: 220 },
        { width: 800, height: 600 },
      ),
    ).toEqual({ x: 100, y: 80, horizontal: "right", vertical: "down" });
  });

  it("flips left and up at the bottom-right viewport edge", () => {
    expect(
      placeRootMenu(
        { x: 790, y: 590 },
        { width: 180, height: 220 },
        { width: 800, height: 600 },
      ),
    ).toEqual({ x: 610, y: 370, horizontal: "left", vertical: "up" });
  });

  it("clamps oversized and non-finite input to the safe margin", () => {
    const placement = placeRootMenu(
      { x: Number.NaN, y: -100 },
      { width: 900, height: 700 },
      { width: 800, height: 600 },
    );
    expect(placement.x).toBe(8);
    expect(placement.y).toBe(8);
  });
});

describe("placeSubmenu", () => {
  it("opens to the right when space is available", () => {
    expect(
      placeSubmenu(
        { left: 100, right: 280, top: 70, bottom: 100 },
        { width: 160, height: 200 },
        { width: 800, height: 600 },
      ),
    ).toEqual({ x: 280, y: 70, horizontal: "right", vertical: "down" });
  });

  it("flips to the left and vertically clamps near the viewport edge", () => {
    expect(
      placeSubmenu(
        { left: 620, right: 792, top: 520, bottom: 550 },
        { width: 180, height: 200 },
        { width: 800, height: 600 },
      ),
    ).toEqual({ x: 440, y: 392, horizontal: "left", vertical: "up" });
  });

  it("chooses the side with more room when neither side fully fits", () => {
    const placement = placeSubmenu(
      { left: 120, right: 300, top: 20, bottom: 50 },
      { width: 500, height: 100 },
      { width: 600, height: 400 },
    );
    expect(placement.horizontal).toBe("right");
    expect(placement.x).toBe(92);
  });
});
