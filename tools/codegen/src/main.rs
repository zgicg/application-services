use std::fs::File;
use std::io::{Read, Error, ErrorKind};

use handlebars::Handlebars;

fn main() -> Result<(), std::io::Error> {
    let path = "./components/push/idl/PushManager.webidl";

    let mut input = File::open(path)?;

    let mut buffer = String::new();
    input.read_to_string(&mut buffer)?;

    let definitions = match weedle::parse(&buffer) {
        Ok(def) => def,
        Err(e) => {
            println!("Error: {}", e);
            return Err(std::io::Error::new(ErrorKind::Other, "oh no!"));
        }
    };

    definitions.render();
    Ok(())
}


trait Render<'src> {
    fn render(&'src self);
}

impl<'src> Render<'src> for [weedle::Definition<'src>] {
    fn render(&'src self) {
        println!("/* Auto-generated fun times! */");
        for defn in self {
            println!("\n");
            defn.render();
        }
    }
}

impl<'src> Render<'src> for weedle::Definition<'src> {
    fn render(&'src self) {
        use weedle::Definition::*;
        match self {
             Interface(interface) => interface.render(),
             _ => panic!("Unimplemented\n{:?}", self),
        }
    }
}

impl<'src> Render<'src> for weedle::InterfaceDefinition<'src> {
    fn render(&'src self) {
        println!("class {} constructor (", self.identifier.0);
        for member in &self.members.body {
            member.render();
        };
        println!(") {{");
        println!("  companion object {{");
        println!("    internal fun fromMessage(msg: MsgTypes.{nm}): {nm} {{", nm=self.identifier.0);
        println!("      return {}(", self.identifier.0);
        for member in &self.members.body {
            match member {
                weedle::interface::InterfaceMember::Attribute(attr) => {
                    println!("        {nm} = msg.{nm},", nm=attr.identifier.0);
                },
                _ => {},
            }
        };
        println!("      )");
        println!("    }}");
        println!("  }}");
        println!("}}");
    }
}

impl<'src> Render<'src> for weedle::interface::InterfaceMember<'src> {
    fn render(&'src self) {
        use weedle::interface::InterfaceMember::*;
        match self {
             Attribute(attr) => attr.render(),
             Operation(op) => op.render(),
             _ => panic!("Unimplemented\n{:?}", self),
        }
    }
}

impl<'src> Render<'src> for weedle::interface::AttributeInterfaceMember<'src> {
    fn render(&'src self) {
        print!("  val {}: ", self.identifier.0);
        self.type_.render();
        println!(",");
    }
}

impl<'src> Render<'src> for weedle::types::AttributedType<'src> {
    fn render(&'src self) {
        self.type_.render();
    }
}

impl<'src> Render<'src> for weedle::types::Type<'src> {
    fn render(&'src self) {
        match self {
            weedle::types::Type::Single(t) => t.render(),
            _ => panic!("Unimplemented {:?}", self),
        }
    }
}

impl<'src> Render<'src> for weedle::types::SingleType<'src> {
    fn render(&'src self) {
        match self {
            weedle::types::SingleType::NonAny(t) => t.render(),
            _ => panic!("Unimplemented {:?}", self),
        }
    }
}

impl<'src> Render<'src> for weedle::types::NonAnyType<'src> {
    fn render(&'src self) {
        match self {
            weedle::types::NonAnyType::Identifier(id) => print!("{}", id.type_.0),
            _ => panic!("Unimplemented {:?}", self),
        }
    }
}

impl<'src> Render<'src> for weedle::interface::OperationInterfaceMember<'src> {
    fn render(&'src self) {
        match self.identifier {
            Some(id) => println!("method {}", id.0),
            None => println!("method with no name"),
        }
    }
}