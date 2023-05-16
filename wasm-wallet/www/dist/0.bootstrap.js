(window["webpackJsonp"] = window["webpackJsonp"] || []).push([[0],{

/***/ "../pkg/dlc_protocol_wallet.js":
/*!*************************************!*\
  !*** ../pkg/dlc_protocol_wallet.js ***!
  \*************************************/
/*! no static exports found */
/***/ (function(module, exports) {

eval("throw new Error(\"Module parse failed: Unexpected token (160:61)\\nYou may need an appropriate loader to handle this file type, currently no loaders are configured to process this file. See https://webpack.js.org/concepts#loaders\\n| async function init(input) {\\n|     if (typeof input === 'undefined') {\\n>         input = new URL('dlc_protocol_wallet_bg.wasm', import.meta.url);\\n|     }\\n|     const imports = getImports();\");\n\n//# sourceURL=webpack:///../pkg/dlc_protocol_wallet.js?");

/***/ }),

/***/ "./index.js":
/*!******************!*\
  !*** ./index.js ***!
  \******************/
/*! no exports provided */
/***/ (function(module, __webpack_exports__, __webpack_require__) {

"use strict";
eval("__webpack_require__.r(__webpack_exports__);\n/* harmony import */ var dlc_protocol_wallet__WEBPACK_IMPORTED_MODULE_0__ = __webpack_require__(/*! dlc_protocol_wallet */ \"../pkg/dlc_protocol_wallet.js\");\n/* harmony import */ var dlc_protocol_wallet__WEBPACK_IMPORTED_MODULE_0___default = /*#__PURE__*/__webpack_require__.n(dlc_protocol_wallet__WEBPACK_IMPORTED_MODULE_0__);\n\n// import { memory } from \"wasm-game-of-life/wasm_game_of_life_bg\";\n\nconst CELL_SIZE = 5; // px\nconst GRID_COLOR = \"#CCCCCC\";\nconst DEAD_COLOR = \"#FFFFFF\";\nconst ALIVE_COLOR = \"#000000\";\n\n// Construct the universe, and get its width and height.\nconst universe = Universe.new();\nconst width = universe.width();\nconst height = universe.height();\n\n// Give the canvas room for all of our cells and a 1px border\n// around each of them.\nconst canvas = document.getElementById(\"game-of-life-canvas\");\ncanvas.height = (CELL_SIZE + 1) * height + 1;\ncanvas.width = (CELL_SIZE + 1) * width + 1;\n\nconst ctx = canvas.getContext('2d');\n\nconst renderLoop = () => {\n    // debugger;\n    universe.tick();\n\n    drawGrid();\n    drawCells();\n\n    requestAnimationFrame(renderLoop);\n};\n\nconst drawGrid = () => {\n    ctx.beginPath();\n    ctx.strokeStyle = GRID_COLOR;\n\n    // Vertical lines.\n    for (let i = 0; i <= width; i++) {\n        ctx.moveTo(i * (CELL_SIZE + 1) + 1, 0);\n        ctx.lineTo(i * (CELL_SIZE + 1) + 1, (CELL_SIZE + 1) * height + 1);\n    }\n\n    // Horizontal lines.\n    for (let j = 0; j <= height; j++) {\n        ctx.moveTo(0, j * (CELL_SIZE + 1) + 1);\n        ctx.lineTo((CELL_SIZE + 1) * width + 1, j * (CELL_SIZE + 1) + 1);\n    }\n\n    ctx.stroke();\n};\n\nconst getIndex = (row, column) => {\n    return row * width + column;\n};\n\nconst drawCells = () => {\n    const cellsPtr = universe.cells();\n    const cells = new Uint8Array(memory.buffer, cellsPtr, width * height);\n\n    ctx.beginPath();\n\n    for (let row = 0; row < height; row++) {\n        for (let col = 0; col < width; col++) {\n            const idx = getIndex(row, col);\n\n            ctx.fillStyle = cells[idx] === Cell.Dead\n                ? DEAD_COLOR\n                : ALIVE_COLOR;\n\n            ctx.fillRect(\n                col * (CELL_SIZE + 1) + 1,\n                row * (CELL_SIZE + 1) + 1,\n                CELL_SIZE,\n                CELL_SIZE\n            );\n        }\n    }\n\n    ctx.stroke();\n};\n\n// run_git_fetch_test().then((res) => console.log(res));\n\ndrawGrid();\ndrawCells();\nrequestAnimationFrame(renderLoop);\n\n\n//# sourceURL=webpack:///./index.js?");

/***/ })

}]);