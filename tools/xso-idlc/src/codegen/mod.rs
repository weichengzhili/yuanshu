pub mod cpp;
pub mod rust;
pub mod doc;
pub mod test;
pub mod anchor;
/// 桥接词汇表：跨语言名称/前缀的单一真相（去硬编码）
pub mod naming;
/// 生成源码的语法词汇表：关键字/属性/声明模板（去硬编码）
pub mod syntax;
/// 结构化发射器：以语义方法产出目标源码（替代 writeln! 拼字符串）
pub mod emitter;