use serde::{Serialize, Deserialize};


// #[derive(Serialize, Deserialize, Debug)]
// #[serde(tag = "type")]
// pub enum MessageFromOrderbook {
//     #[serde(rename = "DEPTH")]
//     Depth {
//         payload: DepthPayload,
//     },
//     #[serde(rename = "ORDER_PLACED")]
//     OrderPlaced {
//         payload: OrderPlacedPayload,
//     },
//     #[serde(rename = "ORDER_CANCELLED")]
//     OrderCancelled {
//         payload: OrderCancelledPayload,
//     },
//     #[serde(rename = "OPEN_ORDERS")]
//     OpenOrders {
//         payload: Vec<OpenOrder>,
//     },
// }



#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MessageFromOrderbook {

    
    DEPTH {
        payload: DepthPayload,
    },
    ORDER_PLACED {
        payload: OrderPlacedPayload,
    },
    ORDER_CANCELLED {
        payload: OrderCancelledPayload,
    },
   #[serde(rename = "OPEN_ORDERS")]
    OpenOrders {
        payload: Vec<OpenOrder>,
    },
    ERROR {  // Add this variant
        message: String,
    },
}







#[derive(Serialize, Deserialize, Debug)]
pub struct DepthPayload {
    pub market: String,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
}

// #[derive(Serialize, Deserialize, Debug)]
// pub struct Fill {
//     pub price: String,
//     pub qty: f64,
//     pub trade_id: i64,
// }






#[derive(Serialize, Deserialize, Debug)]
pub struct Fill {
    #[serde(alias = "marker_order_id")]
    pub order_id: Option<String>,  // Engine uses marker_order_id
    pub other_user_id: Option<String>,
    #[serde(deserialize_with = "deserialize_number_to_string")]
    pub price: String,
    pub qty: f64,
    pub trade_id: i64,
}





#[derive(Serialize, Deserialize, Debug)]
pub struct OrderPlacedPayload {
    pub order_id: String,
    pub executed_qty: f64,
    pub fills: Vec<Fill>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OrderCancelledPayload {
    pub order_id: String,
    pub executed_qty: f64,
    pub remaining_qty: f64,
}






#[derive(Serialize, Deserialize, Debug)]
pub struct OpenOrder {
    pub order_id: String,
    // pub executed_qty: f64,
    pub filled: f64,
    #[serde(deserialize_with = "deserialize_number_to_string")]
    pub price: String,
    #[serde(deserialize_with = "deserialize_number_to_string")]
    pub quantity: String,
    pub side: OrderSide,
    pub user_id: String,
}

fn deserialize_number_to_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, Visitor};
    use std::fmt;

    struct StringOrNumberVisitor;

    impl<'de> Visitor<'de> for StringOrNumberVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number")
        }

        fn visit_str<E>(self, value: &str) -> Result<String, E> {
            Ok(value.to_string())
        }

        fn visit_f64<E>(self, value: f64) -> Result<String, E> {
            Ok(value.to_string())
        }
    }

    deserializer.deserialize_any(StringOrNumberVisitor)
}







// #[derive(Serialize, Deserialize, Debug)]
// pub struct OpenOrder {
//     pub order_id: String,
//     pub executed_qty: f64,
//     pub price: String,
//     pub quantity: String,
//     #[serde(rename = "side")]
//     pub side: OrderSide,
//     pub user_id: String,
// }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum MessageToEngine {
    #[serde(rename = "CREATE_ORDER")]
    CreateOrder {
        data: CreateOrderData,
    },
    #[serde(rename = "CANCEL_ORDER")]
    CancelOrder {
        data: CancelOrderData,

    },
    #[serde(rename = "ON_RAMP")]
    OnRamp {
        data: OnRampData,
    },
    #[serde(rename = "GET_DEPTH")]
    GetDepth {
        data: GetDepthData,
    },
    #[serde(rename = "GET_OPEN_ORDERS")]
    GetOpenOrders {
        data: GetOpenOrdersData,
    },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CreateOrderData {
    pub market: String,
    pub price: String,
    pub quantity: String,
    #[serde(rename = "side")]
    pub side: OrderSide
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CancelOrderData {
    pub order_id: String,
    pub market: String,
   pub user_id: String,  

}

#[derive(Serialize, Deserialize, Debug)]
pub struct OnRampData {
    pub amount: String,
    pub user_id: String,
    pub txn_id: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct GetDepthData {
    pub market: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetOpenOrdersData {
    pub market: String,
        pub user_id: String,  

}

