const { RuleHelper } = require("textlint-rule-helper");

const DEFAULT_PATTERNS = ["TODO:", "TODO ", "FIXME:", "FIXME ", "XXX:", "XXX "];

/**
 * textlint rule: no-todo
 * Reports TODO, FIXME, and XXX markers in text.
 */
function reporter(context) {
  const { Syntax, RuleError, report } = context;
  const helper = new RuleHelper(context);

  return {
    [Syntax.Str](node) {
      if (helper.isChildNode(node, [Syntax.Link, Syntax.Image, Syntax.Code])) {
        return;
      }

      const text = context.getSource(node);
      const patterns = DEFAULT_PATTERNS;

      patterns.forEach((pattern) => {
        let index = text.indexOf(pattern);
        while (index !== -1) {
          const message = `Found '${pattern.trim()}' comment. Consider resolving this before committing.`;
          report(
            node,
            new RuleError(message, {
              index: index,
            })
          );
          index = text.indexOf(pattern, index + 1);
        }
      });
    },
  };
}

module.exports = {
  linter: reporter,
  fixer: reporter,
};
