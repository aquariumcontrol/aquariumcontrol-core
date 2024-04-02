use mysql::*;
use mysql::prelude::*;

/*
#[derive(Debug, PartialEq, Eq)]
struct Car {
    id: i32,
    name: String,
    price: i32,
}
*/

struct TimestampString {
    timestamp: String,    
}

fn testmariadb() -> std::result::Result<std::string::String, mysql::Error> {
    let url = "mysql://demo:demo@vps2483992.servdiscount-customer.com:3307/aquarium";

    let pool = match Pool::new(url)
    {
        Ok(c) => {
            println!("URL pool established.");
            c
        },
        Err(e) => {
            panic!("Could not establish URL pool with {}: {}", url, e);
        }
    };
    
    let mut conn = match pool.get_conn()
    {
        Ok(c) => {
            println!("Database connection opened.");
            c
        },
        // Handle the `error` case.
        Err(_) => {
            panic!("Could not open data base connection.");
        }
    };
    
    /*
    conn.exec_drop(r"delete from cars",())?;

    conn.exec_    
    let cars = vec![
        Car { id: 1, name: "Audi".into(), price: 52642 },
        Car { id: 2, name: "Volkswagen".into(), price: 21600 },
        Car { id: 3, name: "Skoda".into(), price: 9000 },
    ];
    conn.exec_batch(
        r"INSERT INTO cars (id, name, price)
      VALUES (:id, :name, :price)",
      cars.iter().map(|p| params! {
          "id" => p.id,
          "name" => &p.name,
          "price" => p.price,
      })
      )?;
    
    let selected_cars = conn
        .query_map(
            "SELECT id, name, price from cars",
            |(id, name, price)| {
                Car { id, name, price }
            },
            )?;
    assert_eq!(cars, selected_cars);
    */
    println!("Executing query");

    let timestamp_string_array = conn.query_map(
        "SELECT CURRENT_TIMESTAMP();", |timestamp| { TimestampString {timestamp} },
    )?;
    println!("Yay: {}", timestamp_string_array[0].timestamp);
    Ok("Yay!".into())

}

fn main() {
    let _ = testmariadb(); 
}