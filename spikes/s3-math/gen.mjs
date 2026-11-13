// LaTeX → SVG (paths only) with MathJax in Node; writes formulas.json with the box of every formula in ex.
import fs from 'node:fs';
import { createRequire } from 'node:module';
const require = createRequire(import.meta.url);
const { mathjax } = require('mathjax-full/js/mathjax.js');
const { TeX } = require('mathjax-full/js/input/tex.js');
const { SVG } = require('mathjax-full/js/output/svg.js');
const { liteAdaptor } = require('mathjax-full/js/adaptors/liteAdaptor.js');
const { RegisterHTMLHandler } = require('mathjax-full/js/handlers/html.js');
const { AllPackages } = require('mathjax-full/js/input/tex/AllPackages.js');
const adaptor = liteAdaptor(); RegisterHTMLHandler(adaptor);
const doc = mathjax.document('', { InputJax: new TeX({ packages: AllPackages }), OutputJax: new SVG({ fontCache: 'none' }) });
const F = [
  'E = mc^2', 'a^2 + b^2 = c^2', 'x_{1,2} = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}', '\\sum_{i=1}^{n} i = \\frac{n(n+1)}{2}', '\\int_0^1 x^2\\,dx = \\frac{1}{3}',
  '\\lim_{x \\to 0} \\frac{\\sin x}{x} = 1', 'f\'(x) = \\lim_{h \\to 0} \\frac{f(x+h) - f(x)}{h}', '\\vec{F} = m\\vec{a}', 'T(n) = O(n \\log n)', '\\alpha + \\beta = \\gamma',
  '\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}', '\\det A = \\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix} = ad - bc', 'e^{i\\pi} + 1 = 0', '\\sigma = \\sqrt{\\frac{1}{N}\\sum_{i=1}^{N}(x_i - \\mu)^2}', 'P(A|B) = \\frac{P(B|A)P(A)}{P(B)}',
  '\\nabla \\cdot \\vec{E} = \\frac{\\rho}{\\varepsilon_0}', '\\frac{\\partial u}{\\partial t} = a^2 \\frac{\\partial^2 u}{\\partial x^2}', 'y = kx + b', '\\overline{x} = \\frac{1}{n}\\sum x_i', 'R = \\frac{U}{I}',
  '\\prod_{k=1}^{n} k = n!', '\\binom{n}{k} = \\frac{n!}{k!(n-k)!}', 'A \\cap B \\subseteq A \\cup B', '\\forall x \\in \\mathbb{R}\\ \\exists y: y > x', '\\left\\{\\begin{array}{l} x + y = 5 \\\\ x - y = 1 \\end{array}\\right.',
  '\\oint_C \\vec{B}\\cdot d\\vec{l} = \\mu_0 I', '\\hat{H}\\psi = E\\psi', 'v = \\frac{s}{t}', 'v_{\\text{сер}} = \\frac{s_{\\text{заг}}}{t}', '\\eta = \\frac{A_{\\text{кор}}}{A_{\\text{повна}}} \\cdot 100\\%',
];
const out = F.map((latex, i) => {
  const node = doc.convert(latex, { display: i % 5 === 4 });
  const svgNode = adaptor.firstChild(node);
  const svg = adaptor.outerHTML(svgNode);
  const ex = (s) => parseFloat(String(s).replace('ex', ''));
  const style = adaptor.getAttribute(svgNode, 'style') ?? '';
  return { latex, svg, widthEx: ex(adaptor.getAttribute(svgNode, 'width')), heightEx: ex(adaptor.getAttribute(svgNode, 'height')), valignEx: ex((style.match(/vertical-align:\s*(-?[\d.]+)ex/) ?? [0, '0'])[1]), hasText: /<text[ >]/.test(svg) };
});
fs.writeFileSync(new URL('./formulas.json', import.meta.url), JSON.stringify(out, null, 1));
console.log(out.length, 'formulas;', out.filter((f) => f.hasText).length, 'contain <text> (not paths):', out.filter((f) => f.hasText).map((f) => f.latex));
console.log(out[2].svg.slice(0, 260));
